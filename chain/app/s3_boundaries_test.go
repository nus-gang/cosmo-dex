package app

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"reflect"
	"strconv"
	"strings"
	"testing"

	sdkmath "cosmossdk.io/math"

	abci "github.com/cometbft/cometbft/abci/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

func TestS3WithdrawBothOrdersSameAndSeparateBlocks(t *testing.T) {
	for run := 0; run < 3; run++ {
		for _, first := range []bool{true, false} {
			for _, sameBlock := range []bool{true, false} {
				for _, amount := range []string{"100000000", "1"} {
					t.Run(fmt.Sprintf("r%d_withdrawFirst%t_sameBlock%t_atoms%s", run, first, sameBlock, amount), func(t *testing.T) {
						f := newS3Fixture(t, 2, 0, false)
						f.deposit(t, 0, ex.Base, "10000000")
						f.deposit(t, 1, ex.Quote, "100000000")
						wire := f.demo(t)
						settle := f.signS3(t, f.operator, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire}, 0)
						withdraw := f.signS3(t, 1, s2Message(f.fixture, 1, 9, true, ex.Quote, amount, "0"), 0)
						raws := [][]byte{settle, withdraw}
						if first {
							raws = [][]byte{withdraw, settle}
						}
						var results []*abci.ExecTxResult
						if sameBlock {
							results = f.blocks(t, raws...)
						} else {
							results = append(f.blocks(t, raws[0]), f.blocks(t, raws[1])...)
						}
						if first {
							requireCode(t, results[0], "")
							requireCode(t, results[1], "EPOCH_MISMATCH")
							f.balance(t, 0, ex.Quote, "0")
							f.balance(t, 1, ex.Base, "0")
							if f.a.Exchange.S3Epoch(f.ctx(t), f.owner(1)) != 1 {
								t.Fatal("withdraw epoch")
							}
						}
						if !first {
							requireCode(t, results[0], "")
							if amount == "100000000" {
								requireCode(t, results[1], "INSUFFICIENT_CONFIRMED_BALANCE")
								if f.a.Exchange.S3Epoch(f.ctx(t), f.owner(1)) != 0 {
									t.Fatal("failed withdrawal bumped epoch")
								}
								f.balance(t, 1, ex.Quote, "90000000")
								requireCode(t, f.blocks(t, f.signS3(t, 1, s2Message(f.fixture, 1, 10, true, ex.Quote, "90000000", "0"), 0))[0], "")
							} else {
								requireCode(t, results[1], "")
								f.balance(t, 1, ex.Quote, "89999999")
							}
							f.balance(t, 0, ex.Quote, "10000000")
							f.balance(t, 1, ex.Base, "1000000")
						}
						f.snapshot(t)
						if sameBlock && first {
							ctx, e := f.a.CreateQueryContext(4, false)
							mustTest(t, e)
							snap, e := f.a.s3Snapshot(ctx)
							mustTest(t, e)
							events := snap["owner_events"].([]map[string]any)
							if len(events) != 1 || events[0]["tx_index"] != "0" {
								t.Fatal(events)
							}
						}
					})
				}
			}
		}
	}
}

func TestS3SplitFeeAndExistingOrderCapacity(t *testing.T) {
	for run := 0; run < 3; run++ {
		t.Run(fmt.Sprintf("split%d", run), func(t *testing.T) {
			f := newS3Fixture(t, 2, 25, false)
			f.deposit(t, 0, ex.Base, "10000000")
			f.deposit(t, 1, ex.Quote, "100000000")
			s, b := f.proof(t, 0, 1, 2, 2, 401, ^uint64(0)), f.proof(t, 1, 2, 1, 2, 401, ^uint64(0))
			wire := f.batch(t, []map[string]any{s, b}, []map[string]any{f.fill(t, s, b, 1, 401, 1), f.fill(t, s, b, 1, 401, 2)})
			requireCode(t, f.submit(t, wire), "")
			f.balance(t, 0, ex.Quote, "798")
			f.balance(t, 1, ex.Base, "1994")
			if f.a.Exchange.Total(f.ctx(t), ex.Quote).Treasury != "4" || f.a.Exchange.Total(f.ctx(t), ex.Base).Treasury != "6" {
				t.Fatal("per-fill ceiling lost")
			}
		})
	}
	for run := 0; run < 3; run++ {
		t.Run(fmt.Sprintf("reused%d", run), func(t *testing.T) {
			f := newS3Fixture(t, 16, 25, false)
			proofs := []map[string]any{}
			pairs := [][2]map[string]any{}
			for i := 0; i < 16; i += 2 {
				f.deposit(t, i, ex.Base, "1000000000")
				f.deposit(t, i+1, ex.Quote, "1000000000000")
				s, b := f.proof(t, i, uint64(i+1), 2, 1000000, 1000000, ^uint64(0)), f.proof(t, i+1, uint64(i+2), 1, 1000000, 1000000, ^uint64(0))
				proofs = append(proofs, s, b)
				pairs = append(pairs, [2]map[string]any{s, b})
			}
			for batch := 0; batch < 2; batch++ {
				fills := []map[string]any{}
				for i, p := range pairs {
					fills = append(fills, f.fill(t, p[0], p[1], 500000, 1000000, uint64(i+1+batch*8)))
				}
				wire := f.batch(t, proofs, fills)
				r := f.submit(t, wire)
				requireCode(t, r, "")
				if r.GasUsed > 6943982 {
					t.Fatal("existing-record gas bound", r.GasUsed)
				}
				for _, event := range r.Events {
					if event.Type == "exchange_s3_kv" {
						t.Logf("batch %d gas %d KV %v", batch+1, r.GasUsed, event.Attributes)
					}
				}
			}
			f.snapshot(t)
		})
	}
}

func TestS3InjectedCustodyAndCounterFaults(t *testing.T) {
	f := newS3Fixture(t, 2, 0, false)
	f.deposit(t, 0, ex.Base, "10000000")
	f.deposit(t, 1, ex.Quote, "100000000")
	wire := f.demo(t)
	// These impossible-through-public-messages states are intentionally injected
	// only into discarded query caches, not mislabeled as consensus history.
	for _, fault := range []string{"surplus", "deficit", "position_overflow", "owner_epoch_overflow", "operator_epoch_overflow", "last_sequence_overflow"} {
		t.Run(fault, func(t *testing.T) {
			ctx := f.ctx(t).WithBlockHeight(f.h + 1)
			k := f.a.Exchange
			owner := sdk.MustAccAddressFromBech32(f.owner(0))
			module := authtypes.NewModuleAddress(ex.Module)
			switch fault {
			case "surplus":
				mustTest(t, f.a.Bank.SendCoins(ctx, owner, module, sdk.NewCoins(sdk.NewCoin(ex.Base, sdkmath.NewInt(1)))))
				mustTest(t, k.ReconcileAsset(ctx, ex.Base, true))
				if k.Total(ctx, ex.Base).Unassigned != "1" || k.AssetPosition(ctx, f.owner(0), ex.Base).Amount != "10000000" {
					t.Fatal("surplus credited user")
				}
				_, e := k.Withdraw(ctx, s2Message(f.fixture, 0, 9, true, ex.Base, "1", "0").(*ext.MsgWithdraw))
				mustTest(t, e)
				if k.Total(ctx, ex.Base).Unassigned != "1" {
					t.Fatal("withdraw lost quarantine")
				}
			case "deficit":
				mustTest(t, f.a.Bank.SendCoins(ctx, module, owner, sdk.NewCoins(sdk.NewCoin(ex.Base, sdkmath.NewInt(1)))))
				_, e := k.SettleBatch(ctx, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire})
				if e == nil || !strings.Contains(e.Error(), "ASSET_DEFICIT") {
					t.Fatal(e)
				}
			case "position_overflow":
				max := "340282366920938463463374607431768211455"
				k.S3Set(ctx, "a/"+ex.Base+"/"+f.owner(1), []byte(max))
				_, e := k.SettleBatch(ctx, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire})
				if e == nil || !strings.Contains(e.Error(), "INTEGER_RANGE") {
					t.Fatal(e)
				}
			case "owner_epoch_overflow":
				k.S3Set(ctx, "e/"+f.owner(0), []byte(strconv.FormatUint(^uint64(0), 10)))
				_, e := k.BumpOrderEpoch(ctx, &s3.MsgBumpOrderEpoch{Owner: f.owner(0), ExpectedEpoch: ^uint64(0), ExpiryHeight: 100, RequestId: bytes.Repeat([]byte{8}, 32), GenesisHash: f.hash})
				if e == nil || !strings.Contains(e.Error(), "INTEGER_RANGE") {
					t.Fatal(e)
				}
			case "operator_epoch_overflow":
				cfg := k.Config(ctx)
				cfg.Epoch = ^uint64(0)
				raw, _ := json.Marshal(cfg)
				k.S3Set(ctx, "config", raw)
				_, e := k.RotateSettlementOperator(ctx, &s3.MsgRotateSettlementOperator{Authority: f.owner(f.users + 2), ExpectedEpoch: ^uint64(0), NewOperatorPubkey: f.keys[f.users+1].PubKey().Bytes()})
				if e == nil || !strings.Contains(e.Error(), "INTEGER_RANGE") {
					t.Fatal(e)
				}
			case "last_sequence_overflow":
				// Querying the maximal consumed sequence without its receipt must close
				// the path before any wrapped next-sequence can be considered.
				raw, _ := json.Marshal(ex.LastBatch{Seq: ^uint64(0), Hash: strings.Repeat("1", 64)})
				k.S3Set(ctx, "last", raw)
				_, e := k.SettleBatch(ctx, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire})
				if e == nil || !strings.Contains(e.Error(), "RECEIPT_INCONSISTENCY") {
					t.Fatal(e)
				}
			}
		})
	}
}

func TestS3ExpiryCancelRevokeBumpAndBinding(t *testing.T) {
	for run := 0; run < 3; run++ {
		for _, delta := range []int64{-1, 0, 1} {
			t.Run(fmt.Sprintf("r%d_expiry_h%+d", run, delta), func(t *testing.T) {
				f := newS3Fixture(t, 2, 0, false)
				f.deposit(t, 0, ex.Base, "10000000")
				f.deposit(t, 1, ex.Quote, "100000000")
				expiry := uint64(f.h + 1 - delta)
				s, b := f.proof(t, 0, 1, 2, 2000, 10000, expiry), f.proof(t, 1, 2, 1, 1000, 12000, expiry)
				wire := f.batch(t, []map[string]any{s, b}, []map[string]any{f.fill(t, s, b, 1000, 10000, 1)})
				want := "EXPIRED"
				if delta < 0 {
					want = ""
				}
				requireCode(t, f.submit(t, wire), want)
			})
		}
		for _, kind := range []string{"offchain_cancel", "revoke_before", "revoke_after", "bump_before", "id_conflict"} {
			t.Run(fmt.Sprintf("r%d_%s", run, kind), func(t *testing.T) {
				f := newS3Fixture(t, 2, 0, false)
				f.deposit(t, 0, ex.Base, "10000000")
				f.deposit(t, 1, ex.Quote, "100000000")
				s, b := f.proof(t, 0, 1, 2, 2000, 10000, 100), f.proof(t, 1, 2, 1, 2000, 12000, 100)
				wire := f.batch(t, []map[string]any{s, b}, []map[string]any{f.fill(t, s, b, 1000, 10000, 1)})
				ow, e := contract.Encode("OrderV1", s["order"].(map[string]any))
				mustTest(t, e)
				hash, _ := hex.DecodeString(orderHash(t, s))
				msg := sdk.Msg(&s3.MsgRevokeOrder{Owner: f.owner(0), OrderHash: hash, ExpectedEpoch: 0, RequestId: bytes.Repeat([]byte{8}, 32), ExpiryHeight: 100, GenesisHash: f.hash, OrderWire: ow})
				if kind == "offchain_cancel" { // Off-chain cancellation only removes engine R; no chain revocation is sent.
					requireCode(t, f.submit(t, wire), "")
					return
				}
				if kind == "revoke_after" {
					requireCode(t, f.submit(t, wire), "")
				}
				if kind == "bump_before" {
					msg = &s3.MsgBumpOrderEpoch{Owner: f.owner(0), ExpectedEpoch: 0, RequestId: bytes.Repeat([]byte{8}, 32), ExpiryHeight: 100, GenesisHash: f.hash}
				}
				requireCode(t, f.blocks(t, f.signS3(t, 0, msg, 0))[0], "")
				state := f.exchangeState(t)
				requireCode(t, f.blocks(t, f.signS3(t, 0, msg, 0))[0], "")
				if !reflect.DeepEqual(state, f.exchangeState(t)) {
					t.Fatal("control replay mutated state")
				}
				if kind == "revoke_after" {
					requireCode(t, f.submit(t, wire), "")
					wire = f.batch(t, []map[string]any{s, b}, []map[string]any{f.fill(t, s, b, 1000, 10000, 2)})
					requireCode(t, f.submit(t, wire), "ORDER_REVOKED")
					f.balance(t, 0, ex.Quote, "10000000")
					return
				}
				want := "ORDER_REVOKED"
				if kind == "bump_before" {
					want = "EPOCH_MISMATCH"
				}
				if kind == "id_conflict" {
					s["order"].(map[string]any)["expiry_height"] = "101"
					s = f.signOrder(t, 0, s["order"].(map[string]any))
					wire = f.batch(t, []map[string]any{s, b}, []map[string]any{f.fill(t, s, b, 1000, 10000, 2)})
					want = "ID_CONFLICT"
				}
				requireCode(t, f.submit(t, wire), want)
				f.balance(t, 0, ex.Quote, "0")
			})
		}
	}
}

func TestS3SlotsRotationCloseAndConsistency(t *testing.T) {
	for run := 0; run < 3; run++ {
		t.Run(fmt.Sprint(run), func(t *testing.T) {
			f := newS3Fixture(t, 2, 0, false)
			f.deposit(t, 0, ex.Base, "10000000")
			f.deposit(t, 1, ex.Quote, "100000000")
			firstSell, firstBuy := f.proof(t, 0, 1, 2, 2000, 10000, 100), f.proof(t, 1, 2, 1, 2000, 12000, 100)
			wire := f.batch(t, []map[string]any{firstSell, firstBuy}, []map[string]any{f.fill(t, firstSell, firstBuy, 1000, 10000, 1)})
			b, e := ex.DecodeBatch(wire)
			mustTest(t, e)
			for _, bad := range []string{"gap", "previous"} {
				m, e := contract.Decode("BatchV1", wire)
				mustTest(t, e)
				want := "BATCH_SEQUENCE_GAP"
				if bad == "gap" {
					m["batch_seq"] = "2"
				} else {
					m["previous_batch_hash"] = strings.Repeat("1", 64)
					want = "PREVIOUS_BATCH_HASH_MISMATCH"
				}
				before := f.exchangeState(t)
				requireCode(t, f.submit(t, seal(t, m)), want)
				if !reflect.DeepEqual(before, f.exchangeState(t)) {
					t.Fatal("invalid slot changed state")
				}
			}
			requireCode(t, f.submit(t, wire), "")
			old, e := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
			mustTest(t, e)
			m, e := contract.Decode("BatchV1", wire)
			mustTest(t, e)
			m["fills"].([]any)[0].(map[string]any)["quantity_lots"] = "999"
			requireCode(t, f.submit(t, seal(t, m)), "BATCH_CONFLICT")
			next, e := contract.Decode("BatchV1", wire)
			mustTest(t, e)
			next["batch_seq"] = "2"
			next["previous_batch_hash"] = b.Hash
			requireCode(t, f.submit(t, seal(t, next)), "DUPLICATE_FILL")
			rotate := &s3.MsgRotateSettlementOperator{Authority: f.owner(f.users + 2), ExpectedEpoch: 1, NewOperatorPubkey: f.keys[f.users+1].PubKey().Bytes()}
			requireCode(t, f.blocks(t, f.signS3(t, f.users+2, rotate, 0))[0], "")
			requireCode(t, f.submit(t, wire), "OPERATOR_UNAUTHORIZED")
			f.operator = f.users + 1
			requireCode(t, f.submit(t, wire), "")
			s, buy := f.proof(t, 0, 6, 2, 1000, 10000, 100), f.proof(t, 1, 7, 1, 1000, 10000, 100)
			newWire := f.batch(t, []map[string]any{s, buy}, []map[string]any{f.fill(t, s, buy, 1000, 10000, 20)})
			candidate, e := contract.Decode("BatchV1", newWire)
			mustTest(t, e)
			candidate["operator_epoch"] = "1"
			fill := candidate["fills"].([]any)[0].(map[string]any)
			identity := map[string]any{"chain_id": ex.S3ChainID, "market_id": ex.S3Market, "operator_epoch": "1", "command_seq": fill["command_seq"], "match_index": fill["match_index"]}
			idraw, e := contract.Encode("FillIdentityV1", identity)
			mustTest(t, e)
			fill["fill_id"] = ex.S3Hash("NUS/FILL_ID/V1", idraw)
			oldEpoch := seal(t, candidate)
			requireCode(t, f.submit(t, oldEpoch), "OPERATOR_EPOCH_MISMATCH")
			close := &s3.MsgCloseBatch{Operator: f.owner(f.operator), BatchWire: oldEpoch, FailedTxHash: bytes.Repeat([]byte{4}, 32), ResolutionEvidenceHash: bytes.Repeat([]byte{5}, 32)}
			assets := f.snapshot(t)["assets"]
			requireCode(t, f.blocks(t, f.signS3(t, f.operator, close, 0))[0], "")
			requireCode(t, f.blocks(t, f.signS3(t, f.operator, close, 0))[0], "")
			if !reflect.DeepEqual(assets, f.snapshot(t)["assets"]) {
				t.Fatal("close moved assets")
			}
			requireCode(t, f.submit(t, oldEpoch), "BATCH_CLOSED")
			// Closing a past success is a no-op, never a COMMITTED -> VOID transition.
			close.BatchWire = wire
			requireCode(t, f.blocks(t, f.signS3(t, f.operator, close, 0))[0], "")
			receipt, e := f.a.Exchange.StoredReceipt(f.ctx(t), 1, f.a.Exchange.Last(f.ctx(t)))
			mustTest(t, e)
			if !reflect.DeepEqual(old, receipt) {
				t.Fatal("past receipt overwritten")
			}
			nextValid := f.batch(t, []map[string]any{s, buy}, []map[string]any{f.fill(t, s, buy, 1000, 10000, 21)})
			requireCode(t, f.submit(t, nextValid), "")
			requireCode(t, f.submit(t, wire), "")
			// Corruption injection into an isolated query cache: the production keeper
			// must detect a missing historical receipt and digest/content mismatches.
			for _, suffix := range []string{"", "/digest"} {
				ctx := f.ctx(t)
				ctx.KVStore(f.a.Exchange.Key).Delete(f.a.Exchange.S3Key("receipt/1" + suffix))
				if _, e = f.a.Exchange.StoredReceipt(ctx, 1, f.a.Exchange.Last(ctx)); e == nil {
					t.Fatal("missing receipt accepted")
				}
			}
			ctx := f.ctx(t)
			f.a.Exchange.S3Set(ctx, "receipt/1", []byte(`{}`))
			if _, e = f.a.Exchange.StoredReceipt(ctx, 1, f.a.Exchange.Last(ctx)); e == nil {
				t.Fatal("corrupt receipt accepted")
			}
		})
	}
}

func TestS3OrderAuthorizationAndArithmetic(t *testing.T) {
	cases := []struct{ name, want string }{{"signature", "INVALID_SIGNATURE"}, {"key", "ADDRESS_MISMATCH"}, {"unregistered", "ACCOUNT_KEY_UNREGISTERED"}, {"domain", "CONTEXT_MISMATCH"}, {"config", "CONTEXT_MISMATCH"}, {"epoch", "EPOCH_MISMATCH"}, {"fee_cap", "FEE_CAP"}, {"limit", "MARKET_LIMIT"}, {"maker", "MARKET_LIMIT"}, {"qty", "MARKET_LIMIT"}, {"fee_version", "MARKET_LIMIT"}, {"fee_ge_receive", "FEE_GE_RECEIVE"}}
	for run := 0; run < 3; run++ {
		for _, tc := range cases {
			t.Run(fmt.Sprintf("r%d_%s", run, tc.name), func(t *testing.T) {
				f := newS3Fixture(t, 2, 25, false)
				f.deposit(t, 0, ex.Base, "10000000")
				f.deposit(t, 1, ex.Quote, "100000000")
				s, b := f.proof(t, 0, 1, 2, 2000, 10000, 100), f.proof(t, 1, 2, 1, 2000, 12000, 100)
				o := b["order"].(map[string]any)
				switch tc.name {
				case "key":
					o["owner_pubkey"] = base64.StdEncoding.EncodeToString(f.keys[0].PubKey().Bytes())
				case "unregistered":
					o["owner_pubkey"] = base64.StdEncoding.EncodeToString(f.keys[2].PubKey().Bytes())
					o["owner"] = base64.StdEncoding.EncodeToString(f.keys[2].PubKey().Address())
				case "domain":
					o["chain_id"] = "nus-s2-dev-1"
				case "config":
					o["market_config_version"] = "2"
				case "epoch":
					o["owner_epoch"] = "1"
				case "fee_cap":
					o["max_fee_bps"] = "24"
				case "limit":
					o["limit_price_ticks"] = "9999"
				case "qty":
					o["max_qty_lots"] = "1000001"
				}
				b = f.signOrder(t, 1, o)
				if tc.name == "signature" {
					sig := ex.Raw(b, "signature")
					sig[0] ^= 1
					b["signature"] = base64.StdEncoding.EncodeToString(sig)
				}
				q, p := uint64(1000), uint64(10000)
				if tc.name == "fee_ge_receive" {
					q = 1
					p = 1
					s["order"].(map[string]any)["limit_price_ticks"] = "1"
					s = f.signOrder(t, 0, s["order"].(map[string]any))
				}
				fill := f.fill(t, s, b, q, p, 1)
				if tc.name == "maker" {
					fill["maker_order_ref"], fill["taker_order_ref"] = fill["taker_order_ref"], fill["maker_order_ref"]
				}
				if tc.name == "fee_version" {
					fill["fee_policy_version"] = "1"
				}
				wire := f.batch(t, []map[string]any{s, b}, []map[string]any{fill})
				before := f.exchangeState(t)
				requireCode(t, f.submit(t, wire), tc.want)
				if !reflect.DeepEqual(before, f.exchangeState(t)) {
					t.Fatal("authorization failure leaked effects")
				}
			})
		}
	}
}

func TestS3ContractVectorsAndProposalBounds(t *testing.T) {
	for _, name := range []string{"batch-demo-0.bin", "batch-demo-25.bin", "batch-capacity-valid-8-16.bin"} {
		raw, e := os.ReadFile("../../protocol/s3/vectors/" + name)
		mustTest(t, e)
		b, e := ex.DecodeBatch(raw)
		mustTest(t, e)
		encoded, e := contract.Encode("BatchV1", b.Body)
		mustTest(t, e)
		if !bytes.Equal(raw, encoded) {
			t.Fatal("approved wire drift")
		}
	}
	for _, name := range []string{"negative-duplicate-fill.bin", "negative-nine-fills.bin", "negative-seventeen-proofs.bin", "negative-unknown-tag.bin", "negative-empty.bin", "negative-batch-byte-limit.bin", "negative-v1-batch-disabled.bin"} {
		raw, e := os.ReadFile("../../protocol/s3/vectors/" + name)
		mustTest(t, e)
		if _, e = ex.DecodeBatch(raw); e == nil {
			t.Fatalf("negative vector %s accepted", name)
		}
	}
	f := newS3Fixture(t, 2, 0, false)
	wire := f.demo(t)
	raw := f.signS3(t, f.operator, &s3.MsgSettleBatch{Operator: f.owner(f.operator), BatchWire: wire}, 0)
	for _, n := range []int{2, 3} {
		txs := make([][]byte, n)
		for i := range txs {
			txs[i] = raw
		}
		err := f.a.blockBudget(txs)
		if (n == 2) != (err == nil) {
			t.Fatalf("block gas budget %d %v", n, err)
		}
	}
	// 34 small signatures are allowed; the 35th fails independently of gas/bytes.
	userRaw := f.signS3(t, 0, s2Message(f.fixture, 0, 4, false, ex.Base, "1", "0"), 0)
	txs := make([][]byte, 35)
	for i := range txs {
		txs[i] = userRaw
	}
	if f.a.blockBudget(txs[:34]) != nil || f.a.blockBudget(txs) == nil {
		t.Fatal("34 signature bound")
	}
	res, e := f.a.PrepareProposal(&abci.RequestPrepareProposal{Height: f.h + 1, MaxTxBytes: 1048576, Txs: txs})
	mustTest(t, e)
	if len(res.Txs) != 34 {
		t.Fatal("prepare budget", len(res.Txs))
	}
	process, e := f.a.ProcessProposal(&abci.RequestProcessProposal{Height: f.h + 1, Txs: txs})
	mustTest(t, e)
	if process.Status != abci.ResponseProcessProposal_REJECT {
		t.Fatal("process accepted 35")
	}
	if _, e = f.a.FinalizeBlock(&abci.RequestFinalizeBlock{Height: f.h + 1, Txs: txs}); e == nil {
		t.Fatal("finalize accepted 35")
	}
}

func TestS3QueryCanonicalContextAndHistory(t *testing.T) {
	f := newS3Fixture(t, 2, 0, false)
	f.deposit(t, 0, ex.Base, "10000000")
	f.deposit(t, 1, ex.Quote, "100000000")
	wire := f.demo(t)
	requireCode(t, f.submit(t, wire), "")
	query := func(req map[string]any, height int64) *abci.ResponseQuery {
		raw, e := canonicalJSON(req)
		mustTest(t, e)
		r, e := f.a.Query(context.Background(), &abci.RequestQuery{Path: "/nus.exchange.s3.v1.Query/Batch", Data: raw, Height: height})
		mustTest(t, e)
		return r
	}
	input := map[string]any{"context": f.a.Exchange.Context(f.ctx(t)), "height": "3", "batch_seq": "1"}
	r := query(input, 3)
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	var lookup map[string]any
	mustTest(t, json.Unmarshal(r.Value, &lookup))
	if lookup["status"] != "NOT_FOUND_AT_HEIGHT" || lookup["observed_height"] != "3" {
		t.Fatal(lookup)
	}
	input["height"] = "4"
	r = query(input, 4)
	if r.Code != 0 {
		t.Fatal(r.Log)
	}
	mustTest(t, json.Unmarshal(r.Value, &lookup))
	if lookup["status"] != "FOUND" {
		t.Fatal(lookup)
	}
	input["height"] = "03"
	if query(input, 3).Code == 0 {
		t.Fatal("noncanonical height")
	}
	input["height"] = "0"
	input["extra"] = "bad"
	if query(input, 0).Code == 0 {
		t.Fatal("unknown query field")
	}
	delete(input, "extra")
	input["context"].(map[string]string)["genesis_hash"] = strings.Repeat("0", 64)
	if query(input, 0).Code == 0 {
		t.Fatal("wrong genesis query")
	}
	input["context"] = f.a.Exchange.Context(f.ctx(t))
	raw, e := canonicalJSON(input)
	mustTest(t, e)
	raw = append([]byte(" "), raw...)
	r, e = f.a.Query(context.Background(), &abci.RequestQuery{Path: "/nus.exchange.s3.v1.Query/Batch", Data: raw})
	mustTest(t, e)
	if r.Code == 0 {
		t.Fatal("noncanonical JSON accepted")
	}
}
