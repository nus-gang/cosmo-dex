//go:build dev_local_demo

package localdirect

import (
	"bytes"
	"context"
	sdkmath "cosmossdk.io/math"
	"github.com/cosmos/cosmos-sdk/client"
	clienttx "github.com/cosmos/cosmos-sdk/client/tx"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	bank "github.com/cosmos/cosmos-sdk/x/bank/types"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
	"testing"
)

func signed(t *testing.T, cfg client.TxConfig, k *mldsa65.PrivKey, msg sdk.Msg) []byte {
	t.Helper()
	b := cfg.NewTxBuilder()
	if err := b.SetMsgs(msg); err != nil {
		t.Fatal(err)
	}
	b.SetGasLimit(500000)
	b.SetFeeAmount(sdk.NewCoins(sdk.NewCoin("DEVGAS", sdkmath.NewInt(1000))))
	s := signing.SignatureV2{PubKey: k.PubKey(), Data: &signing.SingleSignatureData{SignMode: signing.SignMode_SIGN_MODE_DIRECT}, Sequence: 3}
	if err := b.SetSignatures(s); err != nil {
		t.Fatal(err)
	}
	sig, err := clienttx.SignWithPrivKey(context.Background(), signing.SignMode_SIGN_MODE_DIRECT, authsigning.SignerData{Address: sdk.AccAddress(k.PubKey().Address()).String(), ChainID: "nus-s3-local-demo", AccountNumber: 7, Sequence: 3, PubKey: k.PubKey()}, b, k, cfg, 3)
	if err != nil {
		t.Fatal(err)
	}
	if err = b.SetSignatures(sig); err != nil {
		t.Fatal(err)
	}
	raw, err := cfg.TxEncoder()(b.GetTx())
	if err != nil {
		t.Fatal(err)
	}
	return raw
}
func TestBinding(t *testing.T) {
	_, cfg := app.Encoding()
	k, err := mldsa65.GenPrivKeyFromSeed(bytes.Repeat([]byte{45}, 32))
	if err != nil {
		t.Fatal(err)
	}
	owner := k.PubKey().Address()
	pk := k.PubKey().Bytes()
	g := bytes.Repeat([]byte{1}, 32)
	for _, withdraw := range []bool{false, true} {
		d := &ext.MsgDeposit{Owner: sdk.AccAddress(owner).String(), Denom: "DEVBASE", AmountAtoms: "1", RequestId: bytes.Repeat([]byte{2}, 32), ExpectedEpoch: "0", ExpiryHeight: "100", GenesisHash: g}
		var msg sdk.Msg = d
		if withdraw {
			msg = &ext.MsgWithdraw{Owner: d.Owner, Denom: d.Denom, AmountAtoms: d.AmountAtoms, RequestId: d.RequestId, ExpectedEpoch: d.ExpectedEpoch, ExpiryHeight: d.ExpiryHeight, GenesisHash: d.GenesisHash}
		}
		raw := signed(t, cfg, &k, msg)
		original := bytes.Clone(raw)
		if err := Verify(cfg, raw, owner, pk, g, "nus-s3-local-demo", 7, 3); err != nil {
			t.Fatal(err)
		}
		checks := []struct {
			name              string
			raw, owner, pk, g []byte
			chain             string
			n, s              uint64
		}{
			{"owner", raw, bytes.Repeat([]byte{9}, 20), pk, g, "nus-s3-local-demo", 7, 3},
			{"key", raw, owner, bytes.Repeat([]byte{9}, 1952), g, "nus-s3-local-demo", 7, 3},
			{"genesis", raw, owner, pk, bytes.Repeat([]byte{9}, 32), "nus-s3-local-demo", 7, 3},
			{"chain", raw, owner, pk, g, "other", 7, 3}, {"number", raw, owner, pk, g, "nus-s3-local-demo", 8, 3},
			{"sequence", raw, owner, pk, g, "nus-s3-local-demo", 7, 4}, {"truncated", raw[:len(raw)-1], owner, pk, g, "nus-s3-local-demo", 7, 3},
			{"oversize", make([]byte, 139265), owner, pk, g, "nus-s3-local-demo", 7, 3},
		}
		bad := bytes.Clone(raw)
		bad[len(bad)-1] ^= 1
		checks = append(checks, struct {
			name              string
			raw, owner, pk, g []byte
			chain             string
			n, s              uint64
		}{"signature", bad, owner, pk, g, "nus-s3-local-demo", 7, 3})
		for _, c := range checks {
			if Verify(cfg, c.raw, c.owner, c.pk, c.g, c.chain, c.n, c.s) == nil {
				t.Fatalf("accepted %s", c.name)
			}
		}
		if !bytes.Equal(raw, original) {
			t.Fatal("raw mutated")
		}
	}
}

func TestRejectOtherMessageAndMessageOwner(t *testing.T) {
	_, cfg := app.Encoding()
	k, err := mldsa65.GenPrivKeyFromSeed(bytes.Repeat([]byte{46}, 32))
	if err != nil {
		t.Fatal(err)
	}
	owner := k.PubKey().Address()
	g := bytes.Repeat([]byte{1}, 32)
	for _, msg := range []sdk.Msg{
		&bank.MsgSend{FromAddress: sdk.AccAddress(owner).String(), ToAddress: sdk.AccAddress(bytes.Repeat([]byte{9}, 20)).String(), Amount: sdk.NewCoins(sdk.NewCoin("DEVGAS", sdkmath.NewInt(1)))},
		&ext.MsgDeposit{Owner: sdk.AccAddress(bytes.Repeat([]byte{9}, 20)).String(), Denom: "DEVBASE", AmountAtoms: "1", RequestId: bytes.Repeat([]byte{2}, 32), ExpectedEpoch: "0", ExpiryHeight: "100", GenesisHash: g},
	} {
		raw := signed(t, cfg, &k, msg)
		if Verify(cfg, raw, owner, k.PubKey().Bytes(), g, "nus-s3-local-demo", 7, 3) == nil {
			t.Fatal("accepted wrong message or owner")
		}
	}
}
