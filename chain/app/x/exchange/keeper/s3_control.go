package keeper

import (
	"bytes"
	"context"
	"encoding/hex"
	"strconv"

	sdk "github.com/cosmos/cosmos-sdk/types"
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

func (k Keeper) control(ctx sdk.Context, msg sdk.Msg, owner string, id, genesis []byte, epoch, expiry uint64, fn func() error) error {
	if k.ChainID() != S3ChainID || ctx.ChainID() != S3ChainID || !bytes.Equal(genesis, k.GenesisHash) {
		return S3Error("CONTEXT_MISMATCH")
	}
	addr, e := sdk.AccAddressFromBech32(owner)
	if e != nil || addr.String() != owner || k.S3Get(ctx, "user/"+owner) == nil {
		return S3Error("ACCOUNT_KEY_UNREGISTERED")
	}
	if len(id) != 32 {
		return S3Error("NON_CANONICAL_WIRE")
	}
	raw, e := k.Codec.Marshal(msg)
	if e != nil {
		return e
	}
	hash := S3Hash(sdk.MsgTypeURL(msg), raw)
	rid := hex.EncodeToString(id)
	if r, ok := k.Receipt(ctx, owner, rid); ok {
		if r.RequestHash != hash {
			return S3Error("ID_CONFLICT")
		}
		return nil
	}
	if k.S3Epoch(ctx, owner) != epoch {
		return S3Error("EPOCH_MISMATCH")
	}
	if ctx.BlockHeight() < 1 || uint64(ctx.BlockHeight()) >= expiry {
		return S3Error("EXPIRED")
	}
	if e = fn(); e != nil {
		return e
	}
	after := strconv.FormatUint(k.S3Epoch(ctx, owner), 10)
	kind := "BUMP_EPOCH"
	var order *string
	if revoke, ok := msg.(*s3.MsgRevokeOrder); ok {
		kind = "REVOKE_ORDER"
		h := hex.EncodeToString(revoke.OrderHash)
		order = &h
	}
	r := Receipt{S3ChainID, hex.EncodeToString(genesis), owner, rid, hash, kind, "NONE", "0", strconv.FormatInt(ctx.BlockHeight(), 10), rawHash(ctx.TxBytes()), strconv.FormatUint(epoch, 10), after, "COMMITTED"}
	put(ctx.KVStore(k.Key), []byte("r/"+owner+"/"+rid), r)
	k.S3OwnerEvent(ctx, kind, owner, r.Before, r.After, "NONE", "0", rid, order)
	return nil
}
func (k Keeper) RevokeOrder(c context.Context, m *s3.MsgRevokeOrder) (*s3.MsgResponse, error) {
	e := k.s3Atomic(sdk.UnwrapSDKContext(c), func(ctx sdk.Context) error {
		return k.control(ctx, m, m.Owner, m.RequestId, m.GenesisHash, m.ExpectedEpoch, m.ExpiryHeight, func() error {
			o, e := contract.Decode("OrderV1", m.OrderWire)
			if e != nil {
				return wireError(e)
			}
			if Text(o, "protocol_version") != "1" {
				return S3Error("UNSUPPORTED_VERSION")
			}
			if Text(o, "chain_id") != S3ChainID || Text(o, "genesis_hash") != hex.EncodeToString(k.GenesisHash) || Text(o, "market_id") != S3Market || Text(o, "exchange_module_id") != "x/exchange" || Text(o, "market_config_version") != "1" {
				return S3Error("CONTEXT_MISMATCH")
			}
			if sdk.AccAddress(Raw(o, "owner")).String() != m.Owner || Number(o, "owner_epoch") != m.ExpectedEpoch {
				return S3Error("EPOCH_MISMATCH")
			}
			if !bytes.Equal(k.S3Get(ctx, "user/"+m.Owner), Raw(o, "owner_pubkey")) {
				return S3Error("ACCOUNT_KEY_MISMATCH")
			}
			hash := S3Hash("NUS/ORDER/V1", m.OrderWire)
			if len(m.OrderHash) != 32 || hash != hex.EncodeToString(m.OrderHash) {
				return S3Error("NON_CANONICAL_WIRE")
			}
			key := orderKey(o)
			r, ok := k.loadOrder(ctx, key)
			if ok && (r.Hash != hash || !bytes.Equal(r.Wire, m.OrderWire)) {
				return S3Error("ID_CONFLICT")
			}
			if !ok {
				r = OrderRecord{hash, bytes.Clone(m.OrderWire), nil, "0", false}
			}
			r.Revoked = true
			k.saveOrder(ctx, key, r)
			k.S3Set(ctx, "order_index/"+hash, []byte(key))
			return nil
		})
	})
	if e != nil {
		return nil, e
	}
	return &s3.MsgResponse{}, nil
}
func (k Keeper) BumpOrderEpoch(c context.Context, m *s3.MsgBumpOrderEpoch) (*s3.MsgResponse, error) {
	e := k.s3Atomic(sdk.UnwrapSDKContext(c), func(ctx sdk.Context) error {
		return k.control(ctx, m, m.Owner, m.RequestId, m.GenesisHash, m.ExpectedEpoch, m.ExpiryHeight, func() error {
			if m.ExpectedEpoch == ^uint64(0) {
				return S3Error("INTEGER_RANGE")
			}
			k.S3Set(ctx, "e/"+m.Owner, []byte(strconv.FormatUint(m.ExpectedEpoch+1, 10)))
			return nil
		})
	})
	if e != nil {
		return nil, e
	}
	return &s3.MsgResponse{}, nil
}
func (k Keeper) RotateSettlementOperator(c context.Context, m *s3.MsgRotateSettlementOperator) (*s3.MsgResponse, error) {
	e := k.s3Atomic(sdk.UnwrapSDKContext(c), func(ctx sdk.Context) error {
		if k.ChainID() != S3ChainID || ctx.ChainID() != S3ChainID {
			return S3Error("CONTEXT_MISMATCH")
		}
		cfg := k.Config(ctx)
		if cfg.Admin != m.Authority {
			return S3Error("OPERATOR_UNAUTHORIZED")
		}
		if cfg.Epoch != m.ExpectedEpoch {
			return S3Error("OPERATOR_EPOCH_MISMATCH")
		}
		if cfg.Epoch == ^uint64(0) {
			return S3Error("INTEGER_RANGE")
		}
		addr, e := contract.Address(m.NewOperatorPubkey)
		if e != nil {
			return wireError(e)
		}
		owner := sdk.AccAddress(addr).String()
		if !bytes.Equal(k.S3Get(ctx, "operator/"+owner), m.NewOperatorPubkey) || owner == cfg.Operator {
			return S3Error("OPERATOR_UNAUTHORIZED")
		}
		cfg.Operator = owner
		cfg.Epoch++
		k.s3Put(ctx, "config", cfg)
		ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_operator", sdk.NewAttribute("operator", owner), sdk.NewAttribute("epoch", strconv.FormatUint(cfg.Epoch, 10))))
		return nil
	})
	if e != nil {
		return nil, e
	}
	return &s3.MsgResponse{}, nil
}
