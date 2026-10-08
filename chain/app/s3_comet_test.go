package app

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"cosmossdk.io/log/v2"
	sdkmath "cosmossdk.io/math"
	cmtdb "github.com/cometbft/cometbft-db"
	cmtcfg "github.com/cometbft/cometbft/config"
	cmtjson "github.com/cometbft/cometbft/libs/json"
	cmtlog "github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/node"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/proxy"
	rpcclient "github.com/cometbft/cometbft/rpc/client"
	rpchttp "github.com/cometbft/cometbft/rpc/client/http"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	clienttx "github.com/cosmos/cosmos-sdk/client/tx"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	"github.com/cosmos/cosmos-sdk/server"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	s3 "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/s3types"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

type onceCometDB struct {
	cmtdb.DB
	once sync.Once
	err  error
}

func (d *onceCometDB) Close() error { d.once.Do(func() { d.err = d.DB.Close() }); return d.err }

// Opt-in integration driver. It owns four temporary nodes, fresh random keys,
// loopback ports and a distinct S3 home. It always stops every node on exit.
func TestS3CometLocal(t *testing.T) {
	if os.Getenv("NUS_S3_COMET") != "1" {
		t.Skip("set NUS_S3_COMET=1 to run four local Comet validators")
	}
	for run := 0; run < 3; run++ {
		for _, fee := range []int{0, 25} {
			t.Run(fmt.Sprintf("run%d_fee%d", run, fee), func(t *testing.T) { s3CometRun(t, fee) })
		}
	}
}
func s3CometRun(t *testing.T, fee int) {
	t.Helper()
	mustTest(t, os.MkdirAll(".runtime/s3", 0700))
	root, e := os.MkdirTemp(".runtime/s3", "nus55-")
	mustTest(t, e)
	// Private runtime material stays in this disposable directory, never evidence.
	defer os.RemoveAll(root)
	var configs []*cmtcfg.Config
	var peers []string
	var validators []cmttypes.GenesisValidator
	reserve := []net.Listener{}
	defer func() {
		for _, l := range reserve {
			_ = l.Close()
		}
	}()
	ports := []int{}
	for i := 0; i < 8; i++ {
		l, e := net.Listen("tcp", "127.0.0.1:0")
		mustTest(t, e)
		reserve = append(reserve, l)
		ports = append(ports, l.Addr().(*net.TCPAddr).Port)
	}
	keys := []mldsa65.PrivKey{}
	for i := 0; i < 5; i++ {
		key, e := mldsa65.GenPrivKey()
		mustTest(t, e)
		keys = append(keys, key)
	}
	g := S3Genesis{PublicKeys: [][]byte{keys[0].PubKey().Bytes(), keys[1].PubKey().Bytes()}, OperatorKeys: [][]byte{keys[2].PubKey().Bytes(), keys[3].PubKey().Bytes()}, AdminKey: keys[4].PubKey().Bytes(), FeeBPS: strconv.Itoa(fee), ContractHash: ex.S3ContractHash, ConfigHash: ex.S3ConfigHash}
	if fee == 25 {
		g.ConfigHash = ex.S3Fee25ConfigHash
	}
	state, e := json.Marshal(g)
	mustTest(t, e)
	for i := 0; i < 4; i++ {
		home := filepath.Join(root, fmt.Sprintf("node%d", i))
		cfg := cmtcfg.DefaultConfig().SetRoot(home)
		cfg.Moniker = fmt.Sprintf("s3-b-%d", i)
		cfg.RPC.ListenAddress = fmt.Sprintf("tcp://127.0.0.1:%d", ports[i*2])
		cfg.P2P.ListenAddress = fmt.Sprintf("tcp://127.0.0.1:%d", ports[i*2+1])
		cfg.P2P.AllowDuplicateIP = true
		cfg.P2P.AddrBookStrict = false
		cfg.Consensus.TimeoutCommit = time.Second
		cfg.Instrumentation.Prometheus = false
		cmtcfg.EnsureRoot(home)
		mustTest(t, os.MkdirAll(filepath.Join(home, "data"), 0700))
		pv := privval.LoadOrGenFilePV(cfg.PrivValidatorKeyFile(), cfg.PrivValidatorStateFile())
		pk, e := pv.GetPubKey()
		mustTest(t, e)
		validators = append(validators, cmttypes.GenesisValidator{Address: pk.Address(), PubKey: pk, Power: 10, Name: cfg.Moniker})
		nk, e := p2p.LoadOrGenNodeKey(cfg.NodeKeyFile())
		mustTest(t, e)
		peers = append(peers, fmt.Sprintf("%s@127.0.0.1:%d", nk.ID(), ports[i*2+1]))
		configs = append(configs, cfg)
	}
	params := cmttypes.DefaultConsensusParams()
	params.Block.MaxGas = 20000000
	params.Block.MaxBytes = 1048576
	params.Evidence.MaxBytes = 65536
	genesis := &cmttypes.GenesisDoc{GenesisTime: time.Now().UTC(), ChainID: ex.S3ChainID, InitialHeight: 1, ConsensusParams: params, Validators: validators, AppState: state}
	rawGenesis, e := cmtjson.MarshalIndent(genesis, "", "  ")
	mustTest(t, e)
	genesisHash := sha256.Sum256(rawGenesis)
	for i, cfg := range configs {
		others := []string{}
		for j, p := range peers {
			if i != j {
				others = append(others, p)
			}
		}
		cfg.P2P.PersistentPeers = strings.Join(others, ",")
		mustTest(t, os.WriteFile(cfg.GenesisFile(), rawGenesis, 0600))
		cmtcfg.WriteConfigFile(filepath.Join(cfg.RootDir, "config/config.toml"), cfg)
	}
	var nodes []*node.Node
	var databases []dbm.DB
	var cometDBs []*onceCometDB
	var logs []*os.File
	stop := func() {
		for _, n := range nodes {
			_ = n.Stop()
		}
		for _, n := range nodes {
			n.Wait()
		}
		nodes = nil
		for _, db := range cometDBs {
			_ = db.Close()
		}
		cometDBs = nil
		for _, db := range databases {
			_ = db.Close()
		}
		databases = nil
		for _, l := range logs {
			_ = l.Close()
		}
		logs = nil
	}
	defer stop()
	start := func() {
		for i, cfg := range configs {
			db, e := dbm.NewDB("application", dbm.GoLevelDBBackend, filepath.Join(cfg.RootDir, "data"))
			mustTest(t, e)
			databases = append(databases, db)
			a, e := NewForChain(db, genesisHash[:], log.NewNopLogger(), ex.S3ChainID)
			mustTest(t, e)
			pv := privval.LoadFilePV(cfg.PrivValidatorKeyFile(), cfg.PrivValidatorStateFile())
			nk, e := p2p.LoadNodeKey(cfg.NodeKeyFile())
			mustTest(t, e)
			lf, e := os.OpenFile(filepath.Join(root, fmt.Sprintf("node%d.log", i)), os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0600)
			mustTest(t, e)
			logs = append(logs, lf)
			provider := func(c *cmtcfg.DBContext) (cmtdb.DB, error) {
				db, e := cmtcfg.DefaultDBProvider(c)
				if e != nil {
					return nil, e
				}
				wrapped := &onceCometDB{DB: db}
				cometDBs = append(cometDBs, wrapped)
				return wrapped, nil
			}
			n, e := node.NewNode(cfg, pv, nk, proxy.NewLocalClientCreator(server.NewCometABCIWrapper(a)), node.DefaultGenesisDocProviderFunc(cfg), provider, node.DefaultMetricsProvider(cfg.Instrumentation), cmtlog.NewTMLogger(lf))
			mustTest(t, e)
			nodes = append(nodes, n)
			mustTest(t, n.Start())
		}
	}
	for _, l := range reserve {
		mustTest(t, l.Close())
	}
	reserve = nil
	start()
	transport := &http.Transport{DisableKeepAlives: true}
	defer transport.CloseIdleConnections()
	client, e := rpchttp.NewWithClient(configs[0].RPC.ListenAddress, "/websocket", &http.Client{Transport: transport, Timeout: 10 * time.Second})
	mustTest(t, e)
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	waitHeight := func(min int64) int64 {
		deadline := time.Now().Add(20 * time.Second)
		var lastErr error
		var observed int64
		for {
			status, e := client.Status(ctx)
			lastErr = e
			if e == nil {
				observed = status.SyncInfo.LatestBlockHeight
			}
			if e == nil && status.SyncInfo.LatestBlockHeight >= min {
				// Comet can expose a saved block before SDK Commit finishes. A
				// header height alone is deliberately not our readiness barrier.
				info, err := client.ABCIInfo(ctx)
				if err == nil && info.Response.LastBlockHeight >= min {
					return info.Response.LastBlockHeight
				}
				lastErr = err
			}
			if time.Now().After(deadline) {
				t.Fatalf("wait height %d: observed=%d RPC error=%v", min, observed, lastErr)
			}
			select {
			case <-ctx.Done():
				for i := range configs {
					raw, _ := os.ReadFile(filepath.Join(root, fmt.Sprintf("node%d.log", i)))
					if len(raw) > 5000 {
						raw = raw[len(raw)-5000:]
					}
					t.Log(string(raw))
				}
				t.Fatal(ctx.Err())
				return 0
			case <-time.After(100 * time.Millisecond):
			}
		}
	}
	h := waitHeight(1)
	contextValue := map[string]string{"service_schema": "s3/1", "chain_id": ex.S3ChainID, "genesis_hash": hex.EncodeToString(genesisHash[:]), "contract_hash": g.ContractHash, "config_hash": g.ConfigHash, "market_id": ex.S3Market, "market_config_version": "1"}
	evidence := map[string]any{"genesis_hash": contextValue["genesis_hash"], "fee_bps": g.FeeBPS, "validator_count": "4", "raw_genesis": base64.StdEncoding.EncodeToString(rawGenesis), "txs": []any{}}
	query := func(method string, height int64, extra map[string]any) map[string]any {
		req := map[string]any{"context": contextValue, "height": strconv.FormatInt(height, 10)}
		for k, v := range extra {
			req[k] = v
		}
		raw, e := canonicalJSON(req)
		mustTest(t, e)
		r, e := client.ABCIQueryWithOptions(ctx, "/nus.exchange.s3.v1.Query/"+method, raw, rpcclient.ABCIQueryOptions{Height: height})
		mustTest(t, e)
		if r.Response.Code != 0 {
			t.Fatal(r.Response.Log)
		}
		var out map[string]any
		mustTest(t, json.Unmarshal(r.Response.Value, &out))
		if r.Response.Height != height && height != 0 {
			t.Fatal("query height mismatch")
		}
		return out
	}
	rawRPC := func(method string, height int64) []byte {
		url := strings.Replace(configs[0].RPC.ListenAddress, "tcp://", "http://", 1) + "/" + method + "?height=" + strconv.FormatInt(height, 10)
		req, e := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		mustTest(t, e)
		req.Close = true
		resp, e := http.DefaultClient.Do(req)
		mustTest(t, e)
		defer resp.Body.Close()
		raw, e := io.ReadAll(io.LimitReader(resp.Body, 8*1024*1024+1))
		mustTest(t, e)
		if resp.StatusCode != 200 || len(raw) > 8*1024*1024 {
			t.Fatal("RPC raw response limit/status")
		}
		var envelope struct {
			Result json.RawMessage `json:"result"`
			Error  json.RawMessage `json:"error"`
		}
		mustTest(t, json.Unmarshal(raw, &envelope))
		if len(envelope.Result) == 0 || len(envelope.Error) > 0 {
			t.Fatal("RPC envelope")
		}
		return raw
	}
	sign := func(user int, msg sdk.Msg, gas uint64, feeAtoms int64, timeout uint64) []byte {
		snapshot := query("Snapshot", 0, nil)
		h, _ = strconv.ParseInt(snapshot["height"].(string), 10, 64)
		codec, config := Encoding()
		accountRequest, e := codec.Marshal(&authtypes.QueryAccountRequest{Address: sdk.AccAddress(keys[user].PubKey().Address()).String()})
		mustTest(t, e)
		accountResult, e := client.ABCIQueryWithOptions(ctx, "/cosmos.auth.v1beta1.Query/Account", accountRequest, rpcclient.ABCIQueryOptions{Height: h})
		mustTest(t, e)
		if accountResult.Response.Code != 0 || accountResult.Response.Height != h {
			t.Fatal("account query", accountResult.Response.Log)
		}
		var response authtypes.QueryAccountResponse
		mustTest(t, codec.Unmarshal(accountResult.Response.Value, &response))
		var account sdk.AccountI
		mustTest(t, codec.UnpackAny(response.Account, &account))
		number, seq := account.GetAccountNumber(), account.GetSequence()
		b := config.NewTxBuilder()
		mustTest(t, b.SetMsgs(msg))
		b.SetGasLimit(gas)
		b.SetFeeAmount(sdk.NewCoins(sdk.NewCoin(ex.Gas, sdkmath.NewInt(feeAtoms))))
		if timeout > 0 {
			b.SetTimeoutHeight(uint64(h) + timeout)
		}
		mustTest(t, b.SetSignatures(signing.SignatureV2{PubKey: keys[user].PubKey(), Data: &signing.SingleSignatureData{SignMode: signing.SignMode_SIGN_MODE_DIRECT}, Sequence: seq}))
		sig, e := clienttx.SignWithPrivKey(ctx, signing.SignMode_SIGN_MODE_DIRECT, authsigning.SignerData{Address: sdk.AccAddress(keys[user].PubKey().Address()).String(), ChainID: ex.S3ChainID, AccountNumber: number, Sequence: seq, PubKey: keys[user].PubKey()}, b, &keys[user], config, seq)
		mustTest(t, e)
		mustTest(t, b.SetSignatures(sig))
		raw, e := config.TxEncoder()(b.GetTx())
		mustTest(t, e)
		return raw
	}
	broadcast := func(raw []byte) int64 {
		r, e := client.BroadcastTxCommit(ctx, raw)
		mustTest(t, e)
		if r.CheckTx.Code != 0 || r.TxResult.Code != 0 {
			t.Fatalf("broadcast: check=%s execute=%s", r.CheckTx.Log, r.TxResult.Log)
		}
		height := r.Height
		block, e := client.Block(ctx, &height)
		mustTest(t, e)
		results, e := client.BlockResults(ctx, &height)
		mustTest(t, e)
		index := -1
		for i, tx := range block.Block.Txs {
			if bytes.Equal(tx, raw) {
				index = i
			}
		}
		if index < 0 || index >= len(results.TxsResults) || results.TxsResults[index].Code != 0 {
			t.Fatal("finalized raw inclusion mismatch")
		}
		txhash := sha256.Sum256(raw)
		if !bytes.Equal(txhash[:], r.Hash) {
			t.Fatal("TX hash mismatch")
		}
		blockRaw := rawRPC("block", height)
		resultsRaw := rawRPC("block_results", height)
		evidence["txs"] = append(evidence["txs"].([]any), map[string]any{"raw_tx": base64.StdEncoding.EncodeToString(raw), "tx_hash": hex.EncodeToString(txhash[:]), "height": strconv.FormatInt(height, 10), "tx_index": strconv.Itoa(index), "raw_block": base64.StdEncoding.EncodeToString(blockRaw), "raw_block_results": base64.StdEncoding.EncodeToString(resultsRaw), "gas_used": strconv.FormatInt(results.TxsResults[index].GasUsed, 10)})
		return height
	}
	for user, denom := range []string{ex.Base, ex.Quote} {
		amount := "10000000"
		if user == 1 {
			amount = "100000000"
		}
		m := &ext.MsgDeposit{Owner: sdk.AccAddress(keys[user].PubKey().Address()).String(), Denom: denom, AmountAtoms: amount, RequestId: bytes.Repeat([]byte{1}, 32), ExpectedEpoch: "0", ExpiryHeight: strconv.FormatInt(h+100, 10), GenesisHash: genesisHash[:]}
		broadcast(sign(user, m, 500000, 1000, 0))
	}
	// Build signed V1 orders against the exact random runtime genesis, not A's
	// public fixture genesis. Reuse the already tested wire builder only.
	proof := func(user int, side, qty, limit uint64) map[string]any {
		o := map[string]any{"protocol_version": "1", "chain_id": ex.S3ChainID, "genesis_hash": contextValue["genesis_hash"], "exchange_module_id": "x/exchange", "market_id": ex.S3Market, "market_config_version": "1", "owner": base64.StdEncoding.EncodeToString(keys[user].PubKey().Address()), "owner_pubkey": base64.StdEncoding.EncodeToString(keys[user].PubKey().Bytes()), "order_id": fmt.Sprintf("%064x", user+1), "owner_epoch": "0", "side": strconv.FormatUint(side, 10), "limit_price_ticks": strconv.FormatUint(limit, 10), "max_qty_lots": strconv.FormatUint(qty, 10), "max_fee_bps": "25", "fee_asset_policy_id": "RECEIVE_ASSET_V1", "expiry_height": strconv.FormatInt(h+100, 10), "order_type": "1"}
		wire, e := contract.Encode("OrderV1", o)
		mustTest(t, e)
		sig, e := keys[user].Sign(contract.Frame("NUS/ORDER/V1", wire))
		mustTest(t, e)
		return map[string]any{"order": o, "signature": base64.StdEncoding.EncodeToString(sig)}
	}
	sell, buy := proof(0, 2, 2000, 10000), proof(1, 1, 1000, 12000)
	sr, br := orderHash(t, sell), orderHash(t, buy)
	identity, e := contract.Encode("FillIdentityV1", map[string]any{"chain_id": ex.S3ChainID, "market_id": ex.S3Market, "operator_epoch": "1", "command_seq": "1", "match_index": "0"})
	mustTest(t, e)
	feeVersion := "1"
	if fee == 25 {
		feeVersion = "2"
	}
	fill := map[string]any{"fill_id": ex.S3Hash("NUS/FILL_ID/V1", identity), "maker_order_ref": sr, "taker_order_ref": br, "buyer_order_ref": br, "seller_order_ref": sr, "execution_price_ticks": "10000", "quantity_lots": "1000", "fee_policy_version": feeVersion, "command_seq": "1", "match_index": "0"}
	proofs := []any{sell, buy}
	if sr > br {
		proofs = []any{buy, sell}
	}
	batch := seal(t, map[string]any{"protocol_version": "2", "chain_id": ex.S3ChainID, "market_id": ex.S3Market, "operator_epoch": "1", "batch_seq": "1", "previous_batch_hash": strings.Repeat("0", 64), "new_signed_orders": proofs, "fills": []any{fill}, "genesis_hash": contextValue["genesis_hash"], "exchange_module_id": "x/exchange", "market_config_version": "1"})
	msg := &s3.MsgSettleBatch{Operator: sdk.AccAddress(keys[2].PubKey().Address()).String(), BatchWire: batch}
	settleHeight := broadcast(sign(2, msg, 10000000, 20000, 8))
	receipt := query("Batch", settleHeight, map[string]any{"batch_seq": "1"})
	if receipt["status"] != "FOUND" {
		t.Fatal(receipt)
	}
	snapshot := query("Snapshot", settleHeight, nil)
	evidence["settled_snapshot"] = snapshot
	evidence["receipt"] = receipt
	evidence["batch_wire"] = base64.StdEncoding.EncodeToString(batch)
	netBase, netQuote := "1000000", "10000000"
	if fee == 25 {
		netBase = "997500"
		netQuote = "9975000"
	}
	for _, v := range snapshot["accounts"].([]any) {
		ac := v.(map[string]any)
		for _, asset := range ac["assets"].([]any) {
			a := asset.(map[string]any)
			want := ""
			if ac["owner"] == base64.StdEncoding.EncodeToString(keys[0].PubKey().Address()) {
				want = "9000000"
				if a["denom"] == ex.Quote {
					want = netQuote
				}
			} else {
				want = "90000000"
				if a["denom"] == ex.Base {
					want = netBase
				}
			}
			if a["confirmed_atoms"] != want {
				t.Fatal("settled balance", a, want)
			}
		}
	}
	broadcast(sign(2, msg, 10000000, 20000, 8))
	for user, denom := range []string{ex.Quote, ex.Base} {
		amount := netQuote
		if user == 1 {
			amount = netBase
		}
		m := &ext.MsgWithdraw{Owner: sdk.AccAddress(keys[user].PubKey().Address()).String(), Denom: denom, AmountAtoms: amount, RequestId: bytes.Repeat([]byte{2}, 32), ExpectedEpoch: "0", ExpiryHeight: strconv.FormatInt(h+100, 10), GenesisHash: genesisHash[:]}
		broadcast(sign(user, m, 500000, 1000, 0))
	}
	evidence["withdrawn_snapshot"] = query("Snapshot", 0, nil)
	last := waitHeight(1)
	stop()
	start()
	waitHeight(last + 1)
	recovered := query("Batch", 0, map[string]any{"batch_seq": "1"})
	beforeRaw, _ := canonicalJSON(receipt["receipt"])
	afterRaw, _ := canonicalJSON(recovered["receipt"])
	if !bytes.Equal(beforeRaw, afterRaw) {
		t.Fatal("restart changed original receipt")
	}
	evidence["restart_receipt"] = recovered
	originalHash, e := hex.DecodeString(receipt["receipt"].(map[string]any)["terminal_tx_hash"].(string))
	mustTest(t, e)
	confirmed, e := client.Tx(ctx, originalHash, false)
	mustTest(t, e)
	if confirmed.Height != settleHeight || confirmed.TxResult.Code != 0 || !bytes.Equal(confirmed.Tx.Hash(), originalHash) {
		t.Fatal("restarted finalized TX query mismatch")
	}
	evidence["restart_confirmed_tx"] = map[string]any{"tx_hash": hex.EncodeToString(confirmed.Hash), "height": strconv.FormatInt(confirmed.Height, 10), "tx_index": strconv.FormatUint(uint64(confirmed.Index), 10), "code": "0"}

	if dir := os.Getenv("NUS_S3_EVIDENCE_DIR"); dir != "" {
		dir = filepath.Join(dir, strings.ReplaceAll(t.Name(), "/", "__"))
		mustTest(t, os.MkdirAll(dir, 0700))
		raw, e := json.MarshalIndent(evidence, "", "  ")
		mustTest(t, e)
		mustTest(t, os.WriteFile(filepath.Join(dir, "comet-evidence.json"), raw, 0600))
	}
	t.Logf("four validators fee=%d height=%d txs=%d restart receipt identical", fee, settleHeight, len(evidence["txs"].([]any)))
}
