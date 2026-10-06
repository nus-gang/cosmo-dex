// Offline fixture generator. No service, key generation, signing or broadcast.
package main
import (
 "bytes"
 "encoding/json"
 "os"
 sdk "github.com/cosmos/cosmos-sdk/types"
 codectypes "github.com/cosmos/cosmos-sdk/codec/types"
 "github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
 authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
)
func main() {
 sdk.GetConfig().SetBech32PrefixForAccount("nus", "nuspub")
 pk := &mldsa65.PubKey{Key: bytes.Repeat([]byte{42},1952)}
 base := authtypes.NewBaseAccount(sdk.AccAddress(pk.Address()),pk,9,12)
 wrapped,err := codectypes.NewAnyWithValue(base); if err != nil {panic(err)}
 response := authtypes.QueryAccountResponse{Account:wrapped}
 raw,err := response.Marshal(); if err != nil {panic(err)}
 if err=json.NewEncoder(os.Stdout).Encode(map[string]any{"owner":[]byte(pk.Address()),"account_response":raw});err!=nil {panic(err)}
}
