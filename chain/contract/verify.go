package contract

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"math/big"
	"net/url"
	"strconv"
	"strings"

	"github.com/cloudflare/circl/sign/mldsa/mldsa65"
)

func Frame(domain string, body []byte) []byte {
	b := binary.BigEndian.AppendUint32(nil, uint32(len(domain)))
	b = append(b, domain...)
	b = binary.BigEndian.AppendUint64(b, uint64(len(body)))
	return append(b, body...)
}
func Address(pk []byte) ([]byte, error) {
	if len(pk) != mldsa65.PublicKeySize {
		return nil, Code("KEY_LENGTH")
	}
	h := sha256.Sum256(pk)
	return h[:20], nil
}

// VerifyCrypto checks pure ML-DSA-65 with an empty FIPS context, without policy.
func VerifyCrypto(pk, message, signature []byte) bool {
	if len(pk) != mldsa65.PublicKeySize || len(signature) != mldsa65.SignatureSize {
		return false
	}
	var key mldsa65.PublicKey
	if key.UnmarshalBinary(pk) != nil {
		return false
	}
	return mldsa65.Verify(&key, message, nil, signature)
}

// Context must come from confirmed state, never from the submitted message.
// The caller owns immutable market configuration and atomic state/replay checks.
type Context struct {
	// JSON input must distinguish absent/null binding values from explicit zero.
	heightMissing, epochMissing                                   bool
	SnapshotID                                                    string // Immutable snapshot identity for the rc3 decision port.
	ChainID, GenesisHash, ModuleID, MarketID, MarketConfigVersion string
	RegisteredKey                                                 []byte
	RegisteredKeyType                                             string
	Height, Epoch                                                 uint64
	Revoked                                                       bool
	MaxPrice, MaxQuantity, ActiveFeeBPS                           uint64
	Origin, Audience                                              string
	Now                                                           uint64
	NonceConsumed                                                 bool
}

// UnmarshalJSON preserves binding presence for JSON adapters. Native Go callers
// supply typed values directly; their zero Height/Epoch remain explicit values.
func (c *Context) UnmarshalJSON(data []byte) error {
	type plain Context
	var decoded struct {
		*plain
		Height *uint64
		Epoch  *uint64
	}
	var next Context
	decoded.plain = (*plain)(&next)
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	next.heightMissing = decoded.Height == nil
	next.epochMissing = decoded.Epoch == nil
	if decoded.Height != nil {
		next.Height = *decoded.Height
	}
	if decoded.Epoch != nil {
		next.Epoch = *decoded.Epoch
	}
	*c = next
	return nil
}

var domains = map[string]string{"OrderV1": "NUS/ORDER/V1", "CancelV1": "NUS/CANCEL/V1", "WalletChallengeV1": "NUS/WALLET_AUTH/V1"}

func Expiry(height, expiry uint64) error {
	if height >= expiry {
		return Code("EXPIRED")
	}
	return nil
}
func number(m map[string]any, k string) uint64 {
	n, _ := strconv.ParseUint(m[k].(string), 10, 64)
	return n
}
func raw64(m map[string]any, k string) []byte {
	b, _ := base64.StdEncoding.DecodeString(m[k].(string))
	return b
}

// Verify validates a canonical order/cancel/challenge against a supplied state snapshot.
// Success is authentication/policy acceptance only; it does not reserve funds or consume nonce.
func Verify(name string, body, signature []byte, c Context) error {
	return verify(name, body, signature, c, false)
}

func verify(name string, body, signature []byte, c Context, authOnly bool) error {
	domain, ok := domains[name]
	if !ok {
		return Code("UNSUPPORTED_VERSION")
	}
	m, e := Decode(name, body)
	if e != nil {
		return e
	}
	encoded, e := Encode(name, m)
	if e != nil || !bytes.Equal(encoded, body) {
		return NonCanonical
	}
	if number(m, "protocol_version") != 1 {
		return Code("UNSUPPORTED_VERSION")
	}
	if len(c.GenesisHash) != 64 {
		return Code("CONTEXT_MISMATCH")
	}
	g, e := hex.DecodeString(c.GenesisHash)
	if e != nil || hex.EncodeToString(g) != c.GenesisHash {
		return Code("CONTEXT_MISMATCH")
	}
	if m["chain_id"] != c.ChainID || m["genesis_hash"] != c.GenesisHash {
		return Code("CONTEXT_MISMATCH")
	}
	if name != "WalletChallengeV1" && (m["exchange_module_id"] != c.ModuleID || m["market_id"] != c.MarketID) {
		return Code("CONTEXT_MISMATCH")
	}
	if name == "OrderV1" && m["market_config_version"] != c.MarketConfigVersion {
		return Code("CONTEXT_MISMATCH")
	}
	pk := c.RegisteredKey
	if name == "OrderV1" {
		pk = raw64(m, "owner_pubkey")
	}
	if len(signature) != mldsa65.SignatureSize {
		return Code("KEY_LENGTH")
	}
	if len(pk) == 0 && name != "OrderV1" {
		return Code("ACCOUNT_KEY_UNREGISTERED")
	}
	owner, e := Address(pk)
	if e != nil {
		return e
	}
	if !bytes.Equal(owner, raw64(m, "owner")) {
		return Code("ADDRESS_MISMATCH")
	}
	if len(c.RegisteredKey) == 0 {
		return Code("ACCOUNT_KEY_UNREGISTERED")
	}
	if authOnly && c.RegisteredKeyType == "" {
		return Code("NOT_CONNECTED")
	}
	if c.RegisteredKeyType != "ML-DSA-65" || !bytes.Equal(pk, c.RegisteredKey) {
		return Code("ACCOUNT_KEY_MISMATCH")
	}
	if !VerifyCrypto(pk, Frame(domain, body), signature) {
		return Code("INVALID_SIGNATURE")
	}
	if authOnly {
		return nil
	}
	if name == "WalletChallengeV1" {
		return WalletPolicy(number(m, "issued_at"), number(m, "expiry_time"), c.Now, m["server_origin"].(string), c.Origin, m["audience"].(string), c.Audience, c.NonceConsumed)
	}
	if number(m, "owner_epoch") != c.Epoch {
		return Code("EPOCH_MISMATCH")
	}
	if c.Revoked {
		return Code("ORDER_REVOKED")
	}
	if e = Expiry(c.Height, number(m, "expiry_height")); e != nil {
		return e
	}
	if name == "OrderV1" {
		p, q := number(m, "limit_price_ticks"), number(m, "max_qty_lots")
		if p == 0 || q == 0 || p > c.MaxPrice || q > c.MaxQuantity || number(m, "side") < 1 || number(m, "side") > 2 || number(m, "order_type") < 1 || number(m, "order_type") > 2 || m["fee_asset_policy_id"] != "RECEIVE_ASSET_V1" {
			return Code("MARKET_LIMIT")
		}
		if c.ActiveFeeBPS > 10000 {
			return Code("BPS_RANGE")
		}
		if c.ActiveFeeBPS > number(m, "max_fee_bps") {
			return Code("FEE_CAP")
		}
	}
	return nil
}
func WalletPolicy(issued, expiry, now uint64, origin, allowed, audience, expectedAudience string, consumed bool) error {
	if issued > now || expiry <= issued || expiry-issued > 120 || now >= expiry {
		return Code("EXPIRED")
	}
	u, e := url.Parse(origin)
	if e != nil || u.Scheme != "https" || u.Host == "" || u.User != nil || u.Path != "" || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || strings.Contains(origin, "#") || u.Port() == "443" || strings.ToLower(u.Host) != u.Host || origin != "https://"+u.Host || origin != allowed || audience != expectedAudience || (audience != "private-ws" && audience != "exchange-api") {
		return Code("CONTEXT_MISMATCH")
	}
	if consumed {
		return Code("NONCE_CONSUMED")
	}
	return nil
}

// CheckedArithmetic enforces the intermediate U256 and final U128 boundaries.
func CheckedArithmetic(a, b string, multiply bool) (string, error) {
	x, e := Integer(a, 256)
	if e != nil {
		return "", e
	}
	y, e := Integer(b, 256)
	if e != nil {
		return "", e
	}
	n := new(big.Int)
	if multiply {
		n.Mul(x, y)
	} else {
		n.Add(x, y)
	}
	if n.BitLen() > 256 || n.BitLen() > 128 {
		return "", IntegerRange
	}
	return n.String(), nil
}
func Fee(receive string, bps uint64) (string, error) {
	n, e := Integer(receive, 128)
	if e != nil {
		return "", e
	}
	if bps > 10000 {
		return "", Code("BPS_RANGE")
	}
	if bps == 0 {
		return "0", nil
	}
	f := new(big.Int).Mul(n, new(big.Int).SetUint64(bps))
	f.Add(f, big.NewInt(9999))
	f.Div(f, big.NewInt(10000))
	if f.Cmp(n) >= 0 {
		return "", Code("FEE_GE_RECEIVE")
	}
	if f.BitLen() > 128 {
		return "", IntegerRange
	}
	return f.String(), nil
}
