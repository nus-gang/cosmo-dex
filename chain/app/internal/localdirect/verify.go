//go:build dev_local_demo

// Package localdirect binds a direct user TX to trusted account inputs using
// B's existing SDK decoder and signature handler. It performs no chain effects.
package localdirect

import (
	"bytes"
	"context"
	"errors"
	"github.com/cosmos/cosmos-sdk/client"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
)

// Verify requires account inputs from the same trusted chain observation and
// owner from the authenticated REST session. Success is not CheckTx, settlement,
// freshness or runtime approval. The caller retains the original TX bytes.
func Verify(cfg client.TxConfig, raw, owner, publicKey, genesis []byte, chainID string, number, sequence uint64) error {
	reject := errors.New("DIRECT_TX_REJECTED")
	if len(raw) == 0 || len(raw) > 139264 || len(owner) != 20 || len(publicKey) != 1952 || len(genesis) != 32 || chainID == "" {
		return reject
	}
	tx, err := cfg.TxDecoder()(raw)
	if err != nil {
		return reject
	}
	st, ok := tx.(authsigning.Tx)
	if !ok || st.ValidateBasic() != nil {
		return reject
	}
	msgs := tx.GetMsgs()
	if len(msgs) != 1 {
		return reject
	}
	var address string
	var hash []byte
	switch m := msgs[0].(type) {
	case *ext.MsgDeposit:
		address = m.Owner
		hash = m.GenesisHash
	case *ext.MsgWithdraw:
		address = m.Owner
		hash = m.GenesisHash
	default:
		return reject
	}
	a, err := sdk.AccAddressFromBech32(address)
	if err != nil || !bytes.Equal(a, owner) || !bytes.Equal(hash, genesis) {
		return reject
	}
	signers, err := st.GetSigners()
	if err != nil || len(signers) != 1 || !bytes.Equal(signers[0], owner) {
		return reject
	}
	sigs, err := st.GetSignaturesV2()
	if err != nil || len(sigs) != 1 || sigs[0].Sequence != sequence {
		return reject
	}
	pk, ok := sigs[0].PubKey.(*mldsa65.PubKey)
	if !ok || pk == nil || !bytes.Equal(pk.Bytes(), publicKey) || !bytes.Equal(pk.Address(), owner) {
		return reject
	}
	sig, ok := sigs[0].Data.(*signing.SingleSignatureData)
	if !ok || sig == nil || sig.SignMode != signing.SignMode_SIGN_MODE_DIRECT {
		return reject
	}
	doc, err := authsigning.GetSignBytesAdapter(context.Background(), cfg.SignModeHandler(), sig.SignMode, authsigning.SignerData{Address: address, ChainID: chainID, AccountNumber: number, Sequence: sequence, PubKey: pk}, tx)
	if err != nil || !pk.VerifySignature(doc, sig.Signature) {
		return reject
	}
	return nil
}
