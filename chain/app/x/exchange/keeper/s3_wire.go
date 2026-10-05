package keeper

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"strconv"

	errorsmod "cosmossdk.io/errors"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

const S3ChainID = "nus-s3-dev-1"
const S3Market = "DEVBASE/DEVQUOTE"
const S3ContractHash = "6191a54e268a658e4105943f5d1b28a20c550215cfe51f22b80f1cf74cf4a632"
const S3ConfigHash = "eba02241192cd666104d8767e96203a97d089026b3025e2a3241c9638ebdcb9b"
const S3Fee25ConfigHash = "f8ff238d7c49661f958db397855e951a483c5496dac0511b82b55fa3bd4e900d"
const MaxBatchBytes = 131072
const MaxSettleTxBytes = 139264

var s3Errors = map[string]*errorsmod.Error{}

func init() {
	names := []string{"RESOURCE_LIMIT", "NON_CANONICAL_WIRE", "INTEGER_RANGE", "UNSUPPORTED_VERSION", "CONTEXT_MISMATCH", "OPERATOR_UNAUTHORIZED", "RECEIPT_INCONSISTENCY", "BATCH_CONFLICT", "BATCH_CLOSED", "BATCH_SEQUENCE_GAP", "PREVIOUS_BATCH_HASH_MISMATCH", "OPERATOR_EPOCH_MISMATCH", "KEY_LENGTH", "ADDRESS_MISMATCH", "ACCOUNT_KEY_UNREGISTERED", "ACCOUNT_KEY_MISMATCH", "INVALID_SIGNATURE", "ID_CONFLICT", "EPOCH_MISMATCH", "ORDER_REVOKED", "EXPIRED", "MARKET_LIMIT", "BPS_RANGE", "FEE_CAP", "FEE_GE_RECEIVE", "CUMULATIVE_QTY_EXCEEDED", "INSUFFICIENT_CONFIRMED_BALANCE", "DUPLICATE_FILL", "EMPTY_BATCH", "ASSET_DEFICIT", "KV_BUDGET_EXCEEDED", "SEQUENCE_OVERFLOW", "INVALID_ENVELOPE", "UNSUPPORTED_OPTION"}
	for i, name := range names {
		s3Errors[name] = errorsmod.Register("exchange_s3", uint32(1001+i), name)
	}
}

func S3Error(code string) error {
	if e, ok := s3Errors[code]; ok {
		return e
	}
	return fmt.Errorf("unmapped S3 error: %s", code)
}
func wireError(e error) error {
	if e == nil {
		return nil
	}
	return S3Error(e.Error())
}
func Text(m map[string]any, key string) string { return m[key].(string) }
func Number(m map[string]any, key string) uint64 {
	n, _ := strconv.ParseUint(Text(m, key), 10, 64)
	return n
}
func Raw(m map[string]any, key string) []byte {
	b, _ := base64.StdEncoding.DecodeString(Text(m, key))
	return b
}
func S3Hash(domain string, raw []byte) string {
	h := sha256.Sum256(contract.Frame(domain, raw))
	return hex.EncodeToString(h[:])
}
func rawHash(raw []byte) string { h := sha256.Sum256(raw); return hex.EncodeToString(h[:]) }
func zeroHash() string          { return fmt.Sprintf("%064x", 0) }

type Batch struct {
	Body   map[string]any
	Wire   []byte
	Hash   string
	Orders []any
	Fills  []any
}

// BatchV2 intentionally shares V1's strict field layout, not its hash domain or
// activation policy. Count the top-level arrays before allocating nested maps.
func DecodeBatch(raw []byte) (*Batch, error) {
	if len(raw) > MaxBatchBytes {
		return nil, S3Error("RESOURCE_LIMIT")
	}
	core := make([]byte, 0, len(raw))
	counts := map[uint64]int{}
	for pos := 0; pos < len(raw); {
		start := pos
		key, n := binary.Uvarint(raw[pos:])
		if n <= 0 {
			return nil, S3Error("NON_CANONICAL_WIRE")
		}
		pos += n
		tag := key >> 3
		counts[tag]++
		if counts[8] > 16 || counts[9] > 8 {
			return nil, S3Error("RESOURCE_LIMIT")
		}
		switch key & 7 {
		case 0:
			_, n = binary.Uvarint(raw[pos:])
			if n <= 0 {
				return nil, S3Error("NON_CANONICAL_WIRE")
			}
			pos += n
		case 2:
			size, n := binary.Uvarint(raw[pos:])
			if n <= 0 {
				return nil, S3Error("NON_CANONICAL_WIRE")
			}
			pos += n
			if size > uint64(len(raw)-pos) {
				return nil, S3Error("NON_CANONICAL_WIRE")
			}
			pos += int(size)
		default:
			return nil, S3Error("NON_CANONICAL_WIRE")
		}
		if tag != 7 {
			core = append(core, raw[start:pos]...)
		}
	}
	m, e := contract.Decode("BatchV1", raw)
	if e != nil {
		return nil, wireError(e)
	}
	if Text(m, "protocol_version") != "2" {
		return nil, S3Error("UNSUPPORTED_VERSION")
	}
	canonical, e := contract.Encode("BatchV1", m)
	if e != nil || !bytes.Equal(canonical, raw) {
		return nil, S3Error("NON_CANONICAL_WIRE")
	}
	fills := m["fills"].([]any)
	orders := m["new_signed_orders"].([]any)
	if len(fills) == 0 {
		return nil, S3Error("EMPTY_BATCH")
	}
	if S3Hash("NUS/BATCH_ID/V2", core) != Text(m, "batch_id") {
		return nil, S3Error("NON_CANONICAL_WIRE")
	}
	seen := map[string]bool{}
	var seq, index uint64
	for i, v := range fills {
		f := v.(map[string]any)
		id := Text(f, "fill_id")
		s, j := Number(f, "command_seq"), Number(f, "match_index")
		if seen[id] || i > 0 && (s < seq || s == seq && j <= index) {
			return nil, S3Error("DUPLICATE_FILL")
		}
		seen[id] = true
		seq, index = s, j
		ident := map[string]any{"chain_id": m["chain_id"], "market_id": m["market_id"], "operator_epoch": m["operator_epoch"], "command_seq": f["command_seq"], "match_index": f["match_index"]}
		encoded, e := contract.Encode("FillIdentityV1", ident)
		if e != nil {
			return nil, wireError(e)
		}
		if S3Hash("NUS/FILL_ID/V1", encoded) != id {
			return nil, S3Error("NON_CANONICAL_WIRE")
		}
	}
	previous := ""
	refs := map[string]bool{}
	for _, v := range fills {
		f := v.(map[string]any)
		for _, key := range []string{"maker_order_ref", "taker_order_ref", "buyer_order_ref", "seller_order_ref"} {
			refs[Text(f, key)] = true
		}
	}
	for _, v := range orders {
		proof := v.(map[string]any)
		wire, e := contract.Encode("OrderV1", proof["order"].(map[string]any))
		if e != nil {
			return nil, wireError(e)
		}
		h := S3Hash("NUS/ORDER/V1", wire)
		if h <= previous || !refs[h] {
			return nil, S3Error("NON_CANONICAL_WIRE")
		}
		previous = h
		delete(refs, h)
	}
	if len(refs) != 0 {
		return nil, S3Error("NON_CANONICAL_WIRE")
	}
	return &Batch{m, bytes.Clone(raw), S3Hash("NUS/BATCH_HASH/V2", raw), orders, fills}, nil
}

func (b *Batch) Identity() map[string]any {
	ids := make([]string, 0, len(b.Fills))
	for _, v := range b.Fills {
		ids = append(ids, Text(v.(map[string]any), "fill_id"))
	}
	return map[string]any{"operator_epoch": b.Body["operator_epoch"], "batch_seq": b.Body["batch_seq"], "batch_id": b.Body["batch_id"], "batch_hash": b.Hash, "previous_batch_hash": b.Body["previous_batch_hash"], "fill_ids": ids}
}
