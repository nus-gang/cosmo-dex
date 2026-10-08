package app

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	"cosmossdk.io/log/v2"
	sdkmath "cosmossdk.io/math"
	abci "github.com/cometbft/cometbft/abci/types"
	"github.com/cometbft/cometbft/crypto/ed25519"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	clienttx "github.com/cosmos/cosmos-sdk/client/tx"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	storetypes "github.com/cosmos/cosmos-sdk/store/v2/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

type s3Fixture struct {
	*fixture
	users, operator int
	genesis         []byte
	dir             string
}

func newS3Fixture(t *testing.T, users, fee int, disk bool) *s3Fixture {
	t.Helper()
	f := &s3Fixture{fixture: &fixture{}, users: users, operator: users}
	g := S3Genesis{FeeBPS: strconv.Itoa(fee), ContractHash: ex.S3ContractHash, ConfigHash: ex.S3ConfigHash}
	if fee == 25 {
		g.ConfigHash = ex.S3Fee25ConfigHash
	}
	for i := 0; i < users+3; i++ {
		seed := make([]byte, 32)
		seed[0] = byte(90 + i)
		key, e := mldsa65.GenPrivKeyFromSeed(seed)
		mustTest(t, e)
		f.keys = append(f.keys, key)
		if i < users {
			g.PublicKeys = append(g.PublicKeys, key.PubKey().Bytes())
		} else if i < users+2 {
			g.OperatorKeys = append(g.OperatorKeys, key.PubKey().Bytes())
		} else {
			g.AdminKey = key.PubKey().Bytes()
		}
	}
	state, e := json.Marshal(g)
	mustTest(t, e)
	params := cmttypes.DefaultConsensusParams()
	params.Block.MaxBytes = 1048576
	params.Block.MaxGas = 20000000
	params.Evidence.MaxBytes = 65536
	validators := []cmttypes.GenesisValidator{}
	updates := []abci.ValidatorUpdate{}
	for i := 0; i < 4; i++ {
		pk := ed25519.GenPrivKeyFromSecret([]byte(fmt.Sprintf("s3-mock-validator-%d", i))).PubKey()
		validators = append(validators, cmttypes.GenesisValidator{Address: pk.Address(), PubKey: pk, Power: 10, Name: fmt.Sprintf("v%d", i)})
		updates = append(updates, abci.Ed25519ValidatorUpdate(pk.Bytes(), 10))
	}
	doc := &cmttypes.GenesisDoc{GenesisTime: time.Unix(1700000000, 0).UTC(), ChainID: ex.S3ChainID, InitialHeight: 1, ConsensusParams: params, Validators: validators, AppState: state}
	f.genesis, e = cmtjson.Marshal(doc)
	mustTest(t, e)
	hash := sha256.Sum256(f.genesis)
	f.hash = hash[:]
	if disk {
		f.dir = t.TempDir()
		f.db, e = dbm.NewDB("application", dbm.GoLevelDBBackend, f.dir)
		mustTest(t, e)
	} else {
		f.db = dbm.NewMemDB()
	}
	f.a, e = NewForChain(f.db, f.hash, log.NewNopLogger(), ex.S3ChainID)
	mustTest(t, e)
	protoParams := params.ToProto()
	_, e = f.a.InitChain(&abci.RequestInitChain{ChainId: ex.S3ChainID, InitialHeight: 1, AppStateBytes: state, ConsensusParams: &protoParams, Validators: updates})
	mustTest(t, e)
	f.blocks(t)
	return f
}
func (f *s3Fixture) blocks(t *testing.T, raws ...[]byte) []*abci.ExecTxResult {
	t.Helper()
	f.h++
	hash := sha256.Sum256([]byte(fmt.Sprintf("%x/block/%d", f.hash, f.h)))
	r, e := f.a.FinalizeBlock(&abci.RequestFinalizeBlock{Height: f.h, Time: time.Unix(1700000000+f.h, 0), Txs: raws, Hash: hash[:]})
	mustTest(t, e)
	_, e = f.a.Commit()
	mustTest(t, e)
	if dir := os.Getenv("NUS_S3_EVIDENCE_DIR"); dir != "" {
		name := strings.ReplaceAll(t.Name(), "/", "__")
		dir = filepath.Join(dir, name)
		mustTest(t, os.MkdirAll(dir, 0700))
		mustTest(t, os.WriteFile(filepath.Join(dir, "genesis.json"), f.genesis, 0600))
		value := map[string]any{"genesis_hash": hex.EncodeToString(f.hash), "height": strconv.FormatInt(f.h, 10), "block_hash": hex.EncodeToString(hash[:]), "txs": raws, "results": r.TxResults, "exchange_state": f.exchangeState(t)}
		out, e := json.MarshalIndent(value, "", "  ")
		mustTest(t, e)
		mustTest(t, os.WriteFile(filepath.Join(dir, fmt.Sprintf("block-%03d.json", f.h)), out, 0600))
	}
	return r.TxResults
}
func (f *s3Fixture) signS3(t *testing.T, user int, msg sdk.Msg, offset uint64) []byte {
	t.Helper()
	key := f.keys[user]
	ac := f.a.Auth.GetAccount(f.ctx(t), sdk.AccAddress(key.PubKey().Address()))
	seq := ac.GetSequence() + offset
	b := f.a.TxConfig.NewTxBuilder()
	mustTest(t, b.SetMsgs(msg))
	gas, fee := uint64(500000), int64(1000)
	switch msg.(type) {
	case *s3.MsgSettleBatch:
		gas = 10000000
		fee = 20000
		b.SetTimeoutHeight(uint64(f.h + 8))
	case *s3.MsgCloseBatch:
		gas = 3000000
		fee = 6000
		b.SetTimeoutHeight(uint64(f.h + 8))
	}
	b.SetGasLimit(gas)
	b.SetFeeAmount(sdk.NewCoins(sdk.NewCoin(ex.Gas, sdkmath.NewInt(fee))))
	mustTest(t, b.SetSignatures(signing.SignatureV2{PubKey: key.PubKey(), Data: &signing.SingleSignatureData{SignMode: signing.SignMode_SIGN_MODE_DIRECT}, Sequence: seq}))
	sig, e := clienttx.SignWithPrivKey(context.Background(), signing.SignMode_SIGN_MODE_DIRECT, authsigning.SignerData{Address: ac.GetAddress().String(), ChainID: ex.S3ChainID, AccountNumber: ac.GetAccountNumber(), Sequence: seq, PubKey: key.PubKey()}, b, &key, f.a.TxConfig, seq)
	mustTest(t, e)
	mustTest(t, b.SetSignatures(sig))
	raw, e := f.a.TxConfig.TxEncoder()(b.GetTx())
	mustTest(t, e)
	return raw
}
func (f *s3Fixture) deposit(t *testing.T, user int, denom, amount string) {
	t.Helper()
	msg := s2Message(f.fixture, user, 1, false, denom, amount, "0")
	r := f.blocks(t, f.signS3(t, user, msg, 0))[0]
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
}
func (f *s3Fixture) proof(t *testing.T, user int, id, side, qty, limit, expiry uint64) map[string]any {
	t.Helper()
	o := map[string]any{"protocol_version": "1", "chain_id": ex.S3ChainID, "genesis_hash": hex.EncodeToString(f.hash), "exchange_module_id": "x/exchange", "market_id": ex.S3Market, "market_config_version": "1", "owner": base64.StdEncoding.EncodeToString(f.keys[user].PubKey().Address()), "owner_pubkey": base64.StdEncoding.EncodeToString(f.keys[user].PubKey().Bytes()), "order_id": fmt.Sprintf("%064x", id), "owner_epoch": strconv.FormatUint(f.a.Exchange.S3Epoch(f.ctx(t), f.owner(user)), 10), "side": strconv.FormatUint(side, 10), "limit_price_ticks": strconv.FormatUint(limit, 10), "max_qty_lots": strconv.FormatUint(qty, 10), "max_fee_bps": "4294967295", "fee_asset_policy_id": "RECEIVE_ASSET_V1", "expiry_height": strconv.FormatUint(expiry, 10), "order_type": "1"}
	return f.signOrder(t, user, o)
}
func (f *s3Fixture) signOrder(t *testing.T, user int, o map[string]any) map[string]any {
	t.Helper()
	wire, e := contract.Encode("OrderV1", o)
	mustTest(t, e)
	sig, e := f.keys[user].Sign(contract.Frame("NUS/ORDER/V1", wire))
	mustTest(t, e)
	return map[string]any{"order": o, "signature": base64.StdEncoding.EncodeToString(sig)}
}
func orderHash(t *testing.T, p map[string]any) string {
	t.Helper()
	wire, e := contract.Encode("OrderV1", p["order"].(map[string]any))
	mustTest(t, e)
	return ex.S3Hash("NUS/ORDER/V1", wire)
}
func (f *s3Fixture) fill(t *testing.T, sell, buy map[string]any, q, p, seq uint64) map[string]any {
	t.Helper()
	cfg := f.a.Exchange.Config(f.ctx(t))
	id := map[string]any{"chain_id": ex.S3ChainID, "market_id": ex.S3Market, "operator_epoch": strconv.FormatUint(cfg.Epoch, 10), "command_seq": strconv.FormatUint(seq, 10), "match_index": "0"}
	wire, e := contract.Encode("FillIdentityV1", id)
	mustTest(t, e)
	s, b := orderHash(t, sell), orderHash(t, buy)
	return map[string]any{"fill_id": ex.S3Hash("NUS/FILL_ID/V1", wire), "maker_order_ref": s, "taker_order_ref": b, "buyer_order_ref": b, "seller_order_ref": s, "execution_price_ticks": strconv.FormatUint(p, 10), "quantity_lots": strconv.FormatUint(q, 10), "fee_policy_version": strconv.FormatUint(cfg.FeeVersion, 10), "command_seq": strconv.FormatUint(seq, 10), "match_index": "0"}
}
func seal(t *testing.T, m map[string]any) []byte {
	t.Helper()
	m["batch_id"] = strings.Repeat("0", 64)
	wire, e := contract.Encode("BatchV1", m)
	mustTest(t, e)
	core := []byte{}
	for pos := 0; pos < len(wire); {
		start := pos
		key, n := binary.Uvarint(wire[pos:])
		pos += n
		if key&7 == 0 {
			_, n = binary.Uvarint(wire[pos:])
			pos += n
		} else {
			size, n := binary.Uvarint(wire[pos:])
			pos += n + int(size)
		}
		if key>>3 != 7 {
			core = append(core, wire[start:pos]...)
		}
	}
	m["batch_id"] = ex.S3Hash("NUS/BATCH_ID/V2", core)
	wire, e = contract.Encode("BatchV1", m)
	mustTest(t, e)
	return wire
}
func (f *s3Fixture) batch(t *testing.T, proofs []map[string]any, fills []map[string]any) []byte {
	t.Helper()
	sort.Slice(proofs, func(i, j int) bool { return orderHash(t, proofs[i]) < orderHash(t, proofs[j]) })
	ps, fs := []any{}, []any{}
	for _, p := range proofs {
		ps = append(ps, p)
	}
	for _, x := range fills {
		fs = append(fs, x)
	}
	last := f.a.Exchange.Last(f.ctx(t))
	cfg := f.a.Exchange.Config(f.ctx(t))
	return seal(t, map[string]any{"protocol_version": "2", "chain_id": ex.S3ChainID, "market_id": ex.S3Market, "operator_epoch": strconv.FormatUint(cfg.Epoch, 10), "batch_seq": strconv.FormatUint(last.Seq+1, 10), "previous_batch_hash": last.Hash, "new_signed_orders": ps, "fills": fs, "genesis_hash": hex.EncodeToString(f.hash), "exchange_module_id": "x/exchange", "market_config_version": "1"})
}
func (f *s3Fixture) demo(t *testing.T) []byte {
	t.Helper()
	sell := f.proof(t, 0, 1, 2, 2000, 10000, 100)
	buy := f.proof(t, 1, 2, 1, 1000, 12000, 100)
	return f.batch(t, []map[string]any{sell, buy}, []map[string]any{f.fill(t, sell, buy, 1000, 10000, 1)})
}
func (f *s3Fixture) submit(t *testing.T, wire []byte) *abci.ExecTxResult {
	t.Helper()
	return f.blocks(t, f.signS3(t, f.operator, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire}, 0))[0]
}
func (f *s3Fixture) exchangeState(t *testing.T) map[string]string {
	t.Helper()
	out := map[string]string{}
	it := storetypes.KVStorePrefixIterator(f.ctx(t).KVStore(f.a.Exchange.Key), nil)
	defer it.Close()
	for ; it.Valid(); it.Next() {
		k := string(it.Key())
		if strings.HasSuffix(k, "/height") || strings.HasSuffix(k, "/block_hash") || strings.HasSuffix(k, "/block_time") {
			continue
		}
		out[k] = hex.EncodeToString(it.Value())
	}
	return out
}
func (f *s3Fixture) snapshot(t *testing.T) map[string]any {
	t.Helper()
	v, e := f.a.s3Snapshot(f.ctx(t))
	mustTest(t, e)
	return v
}
func requireCode(t *testing.T, r *abci.ExecTxResult, code string) {
	t.Helper()
	if code == "" {
		if r.Code != 0 {
			t.Fatalf("want success, got %s/%d: %s", r.Codespace, r.Code, r.Log)
		}
	} else if r.Code == 0 || !strings.Contains(r.Log, code) {
		t.Fatalf("want %s got %s/%d %s", code, r.Codespace, r.Code, r.Log)
	}
}
func (f *s3Fixture) balance(t *testing.T, user int, denom, want string) {
	t.Helper()
	if got := f.a.Exchange.AssetPosition(f.ctx(t), f.owner(user), denom).Amount; got != want {
		t.Fatalf("%d %s want %s got %s", user, denom, want, got)
	}
}

func TestS3DemoFeeRetryAndHistoricalReceipt(t *testing.T) {
	for run := 0; run < 3; run++ {
		for _, fee := range []int{0, 25} {
			t.Run(fmt.Sprintf("run%d_fee%d", run, fee), func(t *testing.T) {
				f := newS3Fixture(t, 2, fee, true)
				f.deposit(t, 0, ex.Base, "10000000")
				f.deposit(t, 1, ex.Quote, "100000000")
				wire := f.demo(t)
				raw := f.signS3(t, f.operator, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire}, 0)
				r := f.blocks(t, raw)[0]
				requireCode(t, r, "")
				t.Logf("settle gas=%d tx_bytes=%d", r.GasUsed, len(raw))
				base, quote := "1000000", "10000000"
				if fee == 25 {
					base = "997500"
					quote = "9975000"
				}
				f.balance(t, 0, ex.Base, "9000000")
				f.balance(t, 0, ex.Quote, quote)
				f.balance(t, 1, ex.Base, base)
				f.balance(t, 1, ex.Quote, "90000000")
				f.snapshot(t)
				original, e := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
				mustTest(t, e)
				before := f.exchangeState(t)
				r = f.blocks(t, raw)[0]
				if r.Code == 0 {
					t.Fatal("raw sequence replay succeeded")
				}
				if !reflect.DeepEqual(before, f.exchangeState(t)) {
					t.Fatal("raw replay mutated exchange")
				}
				requireCode(t, f.submit(t, wire), "")
				if !reflect.DeepEqual(before, f.exchangeState(t)) {
					t.Fatal("re-envelope applied twice")
				}
				for user, denom := range []string{ex.Quote, ex.Base} {
					amount := quote
					if user == 1 {
						amount = base
					}
					msg := s2Message(f.fixture, user, 9, true, denom, amount, "0")
					requireCode(t, f.blocks(t, f.signS3(t, user, msg, 0))[0], "")
					f.balance(t, user, denom, "0")
				}
				f.snapshot(t)
				mustTest(t, f.db.Close())
				f.db, e = dbm.NewDB("application", dbm.GoLevelDBBackend, f.dir)
				mustTest(t, e)
				f.a, e = NewForChain(f.db, f.hash, log.NewNopLogger(), ex.S3ChainID)
				mustTest(t, e)
				after, e := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
				mustTest(t, e)
				if !reflect.DeepEqual(original, after) {
					t.Fatal("restart lost original receipt")
				}
				f.snapshot(t)
				request, e := canonicalJSON(map[string]any{"context": f.a.Exchange.Context(f.ctx(t)), "height": "0", "batch_seq": "1"})
				mustTest(t, e)
				res, e := f.a.Query(context.Background(), &abci.RequestQuery{Path: "/nus.exchange.s3.v1.Query/Batch", Data: request})
				mustTest(t, e)
				if res.Code != 0 {
					t.Fatal(res.Log)
				}
				var lookup map[string]any
				mustTest(t, json.Unmarshal(res.Value, &lookup))
				if lookup["status"] != "FOUND" {
					t.Fatal(lookup)
				}
				mustTest(t, f.db.Close())
			})
		}
	}
}

func TestS3AtomicRollbackAndGrossDebit(t *testing.T) {
	for run := 0; run < 3; run++ {
		for _, failure := range []string{"signature", "cumulative", "balance"} {
			for _, badFirst := range []bool{false, true} {
				t.Run(fmt.Sprintf("run%d_%s_badFirst%t", run, failure, badFirst), func(t *testing.T) {
					f := newS3Fixture(t, 4, 0, false)
					f.deposit(t, 0, ex.Base, "10000000")
					f.deposit(t, 1, ex.Quote, "100000000")
					f.deposit(t, 2, ex.Base, "10000000")
					if failure != "balance" {
						f.deposit(t, 3, ex.Quote, "100000000")
					}
					s1, b1 := f.proof(t, 0, 1, 2, 1000, 10000, 100), f.proof(t, 1, 2, 1, 1000, 10000, 100)
					s2, b2 := f.proof(t, 2, 3, 2, 1000, 10000, 100), f.proof(t, 3, 4, 1, 1000, 10000, 100)
					qty := uint64(1000)
					want := "INSUFFICIENT_CONFIRMED_BALANCE"
					if failure == "cumulative" {
						qty = 1001
						want = "CUMULATIVE_QTY_EXCEEDED"
					}
					if failure == "signature" {
						sig := ex.Raw(b2, "signature")
						sig[20] ^= 1
						b2["signature"] = base64.StdEncoding.EncodeToString(sig)
						want = "INVALID_SIGNATURE"
					}
					goodSeq, badSeq := uint64(1), uint64(2)
					if badFirst {
						goodSeq, badSeq = 2, 1
					}
					good, bad := f.fill(t, s1, b1, 1000, 10000, goodSeq), f.fill(t, s2, b2, qty, 10000, badSeq)
					fills := []map[string]any{good, bad}
					if badFirst {
						fills = []map[string]any{bad, good}
					}
					wire := f.batch(t, []map[string]any{s1, b1, s2, b2}, fills)
					before := f.exchangeState(t)
					gas := f.a.Bank.GetBalance(f.ctx(t), sdk.MustAccAddressFromBech32(f.owner(f.operator)), ex.Gas).Amount
					requireCode(t, f.submit(t, wire), want)
					if !reflect.DeepEqual(before, f.exchangeState(t)) {
						t.Fatal("partial exchange effects escaped failed message")
					}
					afterGas := f.a.Bank.GetBalance(f.ctx(t), sdk.MustAccAddressFromBech32(f.owner(f.operator)), ex.Gas).Amount
					if !gas.Sub(afterGas).Equal(sdkmath.NewInt(20000)) {
						t.Fatal("failed message ante fee missing")
					}
					f.snapshot(t)
				})
			}
		}
	}
	for run := 0; run < 3; run++ {
		t.Run(fmt.Sprintf("cycle%d", run), func(t *testing.T) {
			f := newS3Fixture(t, 2, 0, false)
			f.deposit(t, 0, ex.Base, "1000")
			f.deposit(t, 1, ex.Quote, "10000")
			s1, b1 := f.proof(t, 0, 1, 2, 1, 10000, 100), f.proof(t, 1, 2, 1, 1, 10000, 100)
			s2, b2 := f.proof(t, 1, 3, 2, 1, 10000, 100), f.proof(t, 0, 4, 1, 1, 10000, 100)
			wire := f.batch(t, []map[string]any{s1, b1, s2, b2}, []map[string]any{f.fill(t, s1, b1, 1, 10000, 1), f.fill(t, s2, b2, 1, 10000, 2)})
			before := f.exchangeState(t)
			requireCode(t, f.submit(t, wire), "INSUFFICIENT_CONFIRMED_BALANCE")
			if !reflect.DeepEqual(before, f.exchangeState(t)) {
				t.Fatal("cycle mutated ledger")
			}
		})
	}
}

func TestS3CapacityActualGasAndKV(t *testing.T) {
	for run := 0; run < 3; run++ {
		t.Run(fmt.Sprint(run), func(t *testing.T) {
			f := newS3Fixture(t, 16, 25, false)
			proofs, fills := []map[string]any{}, []map[string]any{}
			for i := 0; i < 16; i += 2 {
				f.deposit(t, i, ex.Base, "1000000000")
				f.deposit(t, i+1, ex.Quote, "1000000000000")
				s, b := f.proof(t, i, uint64(i+1), 2, 1000000, 1000000, 1000), f.proof(t, i+1, uint64(i+2), 1, 1000000, 1000000, 1000)
				proofs = append(proofs, s, b)
				fills = append(fills, f.fill(t, s, b, 1000000, 1000000, uint64(i+1)))
			}
			wire := f.batch(t, proofs, fills)
			raw := f.signS3(t, f.operator, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire}, 0)
			r := f.blocks(t, raw)[0]
			requireCode(t, r, "")
			if r.GasUsed > 6943982 {
				t.Fatalf("gas bound exceeded %d", r.GasUsed)
			}
			traced := false
			for _, event := range r.Events {
				if event.Type == "exchange_s3_kv" {
					traced = true
					t.Logf("capacity batch_bytes=%d tx_bytes=%d actual_gas=%d KV=%v", len(wire), len(raw), r.GasUsed, event.Attributes)
				}
			}
			if !traced {
				t.Fatal("missing KV trace")
			}
			f.snapshot(t)
		})
	}
}

var _ = authtypes.FeeCollectorName
