// nusd is a local synthetic-asset S1 development chain. Test keys are public.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"syscall"
	"time"

	"cosmossdk.io/log/v2"
	sdkmath "cosmossdk.io/math"
	cmtcfg "github.com/cometbft/cometbft/config"
	cmtlog "github.com/cometbft/cometbft/libs/log"
	"github.com/cometbft/cometbft/node"
	"github.com/cometbft/cometbft/p2p"
	"github.com/cometbft/cometbft/privval"
	"github.com/cometbft/cometbft/proxy"
	rpchttp "github.com/cometbft/cometbft/rpc/client/http"
	cmttypes "github.com/cometbft/cometbft/types"
	dbm "github.com/cosmos/cosmos-db"
	clienttx "github.com/cosmos/cosmos-sdk/client/tx"
	"github.com/cosmos/cosmos-sdk/crypto/keys/mldsa65"
	"github.com/cosmos/cosmos-sdk/server"
	sdk "github.com/cosmos/cosmos-sdk/types"
	"github.com/cosmos/cosmos-sdk/types/tx/signing"
	authsigning "github.com/cosmos/cosmos-sdk/x/auth/signing"
	app "github.com/nus-gang/cosmo-dex/chain/app"
	ex "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/keeper"
	ext "github.com/nus-gang/cosmo-dex/chain/app/x/exchange/types"
	"github.com/spf13/viper"
)

var buildCommit = "development"

func must(e error) {
	if e != nil {
		fmt.Fprintln(os.Stderr, e)
		os.Exit(1)
	}
}
func emit(v any) { b, e := json.MarshalIndent(v, "", "  "); must(e); fmt.Println(string(b)) }
func key(user int) mldsa65.PrivKey {
	if user < 0 || user > 1 {
		must(fmt.Errorf("test user must be 0 or 1"))
	}
	seed := make([]byte, 32)
	seed[0] = byte(user + 1)
	k, e := mldsa65.GenPrivKeyFromSeed(seed)
	must(e)
	return k
}
func config(home string, rpc, p2paddr string) *cmtcfg.Config {
	c := cmtcfg.DefaultConfig().SetRoot(home)
	c.RPC.ListenAddress = rpc
	c.P2P.ListenAddress = p2paddr
	c.Consensus.TimeoutCommit = time.Second
	return c
}
func main() {
	if len(os.Args) < 2 {
		must(fmt.Errorf("usage: nusd init|start|snapshot|receipt|tx|broadcast|version [flags]"))
	}
	cmd := os.Args[1]
	f := flag.NewFlagSet(cmd, flag.ExitOnError)
	home := f.String("home", ".nus-s1", "node directory")
	rpc := f.String("rpc", "tcp://127.0.0.1:26657", "local RPC endpoint")
	p2paddr := f.String("p2p", "tcp://127.0.0.1:26656", "P2P listen address")
	user := f.Int("user", 0, "public test key 0 or 1")
	op := f.String("op", "deposit", "deposit or withdraw")
	amount := f.String("amount", "1000000", "integer atoms")
	id := f.String("request-id", "", "32 byte hex id (required)")
	epoch := f.String("epoch", "", "expected epoch, default committed query")
	expiry := f.String("expiry", "", "exclusive expiry, default height + 100")
	output := f.String("out", "", "write signed TxRaw without submitting")
	file := f.String("file", "", "signed TxRaw file")
	hashFlag := f.String("genesis-hash", "", "required pinned genesis hash for start")
	must(f.Parse(os.Args[2:]))
	c, txcfg := app.Encoding()
	if cmd == "version" {
		emit(map[string]string{"app": app.Version, "sdk": "v0.55.0", "comet": "v0.40.0", "execution_sha": buildCommit})
		return
	}
	cfg := config(*home, *rpc, *p2paddr)
	if cmd == "init" {
		if _, e := os.Stat(cfg.GenesisFile()); !os.IsNotExist(e) {
			must(fmt.Errorf("home already initialized or inaccessible"))
		}
		cmtcfg.EnsureRoot(*home)
		must(os.MkdirAll(filepath.Join(*home, "data"), 0700))
		pv := privval.LoadOrGenFilePV(cfg.PrivValidatorKeyFile(), cfg.PrivValidatorStateFile())
		pub, e := pv.GetPubKey()
		must(e)
		_, e = p2p.LoadOrGenNodeKey(cfg.NodeKeyFile())
		must(e)
		state, e := json.Marshal(app.Genesis{PublicKeys: [][]byte{key(0).PubKey().Bytes(), key(1).PubKey().Bytes()}})
		must(e)
		g := &cmttypes.GenesisDoc{GenesisTime: time.Now().UTC(), ChainID: ex.ChainID, InitialHeight: 1, ConsensusParams: cmttypes.DefaultConsensusParams(), Validators: []cmttypes.GenesisValidator{{Address: pub.Address(), PubKey: pub, Power: 10, Name: "local-validator"}}, AppState: state}
		must(g.SaveAs(cfg.GenesisFile()))
		cmtcfg.WriteConfigFile(filepath.Join(*home, "config/config.toml"), cfg)
		raw, e := os.ReadFile(cfg.GenesisFile())
		must(e)
		h := sha256.Sum256(raw)
		emit(map[string]any{"genesis_hash": hex.EncodeToString(h[:]), "users": []string{sdk.AccAddress(key(0).PubKey().Address()).String(), sdk.AccAddress(key(1).PubKey().Address()).String()}, "warning": "PUBLIC SYNTHETIC TEST KEYS; single validator smoke, not AT01"})
		return
	}
	if cmd == "start" {
		v := viper.New()
		v.SetConfigFile(filepath.Join(*home, "config/config.toml"))
		must(v.ReadInConfig())
		must(v.Unmarshal(cfg))
		cfg.SetRoot(*home)
		f.Visit(func(fl *flag.Flag) {
			switch fl.Name {
			case "rpc":
				cfg.RPC.ListenAddress = *rpc
			case "p2p":
				cfg.P2P.ListenAddress = *p2paddr
			}
		})
		must(cfg.ValidateBasic())
		raw, e := os.ReadFile(cfg.GenesisFile())
		must(e)
		h := sha256.Sum256(raw)
		if *hashFlag != hex.EncodeToString(h[:]) {
			must(fmt.Errorf("missing or mismatched --genesis-hash"))
		}
		db, e := dbm.NewDB("application", dbm.GoLevelDBBackend, filepath.Join(*home, "data"))
		must(e)
		defer db.Close()
		a, e := app.New(db, h[:], log.NewLogger(os.Stderr))
		must(e)
		pv := privval.LoadFilePV(cfg.PrivValidatorKeyFile(), cfg.PrivValidatorStateFile())
		nk, e := p2p.LoadNodeKey(cfg.NodeKeyFile())
		must(e)
		n, e := node.NewNode(cfg, pv, nk, proxy.NewLocalClientCreator(server.NewCometABCIWrapper(a)), node.DefaultGenesisDocProviderFunc(cfg), cmtcfg.DefaultDBProvider, node.DefaultMetricsProvider(cfg.Instrumentation), cmtlog.NewTMLogger(cmtlog.NewSyncWriter(os.Stderr)))
		must(e)
		must(n.Start())
		ch := make(chan os.Signal, 1)
		signal.Notify(ch, syscall.SIGINT, syscall.SIGTERM)
		<-ch
		must(n.Stop())
		n.Wait()
		return
	}
	cli, e := rpchttp.New(*rpc, "/websocket")
	must(e)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	query := func(path string, req interface{ Marshal() ([]byte, error) }) []byte {
		b, e := req.Marshal()
		must(e)
		r, e := cli.ABCIQuery(ctx, path, b)
		must(e)
		if r.Response.Code != 0 {
			must(fmt.Errorf("query code %d: %s", r.Response.Code, r.Response.Log))
		}
		var v ext.QueryJSONResponse
		must(c.Unmarshal(r.Response.Value, &v))
		return v.Json
	}
	if cmd == "snapshot" {
		fmt.Println(string(query("/nus.exchange.v1.Query/Snapshot", &ext.QuerySnapshotRequest{})))
		return
	}
	if cmd == "receipt" {
		owner := sdk.AccAddress(key(*user).PubKey().Address()).String()
		fmt.Println(string(query("/nus.exchange.v1.Query/Receipt", &ext.QueryReceiptRequest{Owner: owner, RequestId: *id})))
		return
	}
	var raw []byte
	if cmd == "broadcast" {
		raw, e = os.ReadFile(*file)
		must(e)
	} else if cmd == "tx" {
		rawID, e := hex.DecodeString(*id)
		must(e)
		if len(rawID) != 32 {
			must(fmt.Errorf("--request-id must be 64 hex characters"))
		}
		k := key(*user)
		owner := sdk.AccAddress(k.PubKey().Address()).String()
		var snap struct {
			Height   string `json:"observed_height"`
			Hash     string `json:"genesis_hash"`
			Accounts []struct {
				Owner    string `json:"owner"`
				Number   string `json:"account_number"`
				Sequence string `json:"sequence"`
				Epoch    string `json:"epoch"`
			} `json:"accounts"`
		}
		must(json.Unmarshal(query("/nus.exchange.v1.Query/Snapshot", &ext.QuerySnapshotRequest{}), &snap))
		var number, seq uint64
		found := false
		for _, ac := range snap.Accounts {
			if ac.Owner == owner {
				number, e = strconv.ParseUint(ac.Number, 10, 64)
				must(e)
				seq, e = strconv.ParseUint(ac.Sequence, 10, 64)
				must(e)
				if *epoch == "" {
					*epoch = ac.Epoch
				}
				found = true
			}
		}
		if !found {
			must(fmt.Errorf("account not registered"))
		}
		gh, e := hex.DecodeString(snap.Hash)
		must(e)
		if *expiry == "" {
			h, e := strconv.ParseUint(snap.Height, 10, 64)
			must(e)
			*expiry = strconv.FormatUint(h+100, 10)
		}
		b := txcfg.NewTxBuilder()
		var m sdk.Msg
		if *op == "deposit" {
			m = &ext.MsgDeposit{Owner: owner, Denom: ex.Quote, AmountAtoms: *amount, RequestId: rawID, ExpectedEpoch: *epoch, ExpiryHeight: *expiry, GenesisHash: gh}
		} else if *op == "withdraw" {
			m = &ext.MsgWithdraw{Owner: owner, Denom: ex.Quote, AmountAtoms: *amount, RequestId: rawID, ExpectedEpoch: *epoch, ExpiryHeight: *expiry, GenesisHash: gh}
		} else {
			must(fmt.Errorf("invalid operation"))
		}
		must(b.SetMsgs(m))
		b.SetGasLimit(500000)
		b.SetFeeAmount(sdk.NewCoins(sdk.NewCoin(ex.Gas, sdkmath.NewInt(1000))))
		must(b.SetSignatures(signing.SignatureV2{PubKey: k.PubKey(), Data: &signing.SingleSignatureData{SignMode: signing.SignMode_SIGN_MODE_DIRECT}, Sequence: seq}))
		sig, e := clienttx.SignWithPrivKey(ctx, signing.SignMode_SIGN_MODE_DIRECT, authsigning.SignerData{Address: owner, ChainID: ex.ChainID, AccountNumber: number, Sequence: seq, PubKey: k.PubKey()}, b, &k, txcfg, seq)
		must(e)
		must(b.SetSignatures(sig))
		raw, e = txcfg.TxEncoder()(b.GetTx())
		must(e)
		if *output != "" {
			must(os.WriteFile(*output, raw, 0600))
			h := sha256.Sum256(raw)
			emit(map[string]any{"tx_hash": fmt.Sprintf("%X", h), "state": "SIGNED_NOT_SUBMITTED"})
			return
		}
	} else {
		must(fmt.Errorf("unknown command"))
	}
	r, e := cli.BroadcastTxCommit(ctx, raw)
	must(e)
	emit(r)
	if r.CheckTx.Code != 0 || r.TxResult.Code != 0 || r.Height <= 0 {
		os.Exit(2)
	}
}
