// Test-only adapter; uses public synthetic seed, never production keys.
package main
import("encoding/json";"encoding/hex";"bufio";"os";"fmt";"github.com/nus-gang/cosmo-dex/chain/contract";"github.com/cloudflare/circl/sign/mldsa/mldsa65")
func h(s string)[]byte{b,e:=hex.DecodeString(s);if e!=nil{panic(e)};return b}
type R struct{Op,Name,Wire,PK,Sig,Msg,Seed,Context,Domain,Receive,Rate,Cap string; API json.RawMessage; BPS uint64; C contract.Context; Snapshot *contract.Snapshot; Auth contract.Result}
func run(r R)map[string]any{o:=map[string]any{"code":"OK"};var e error
switch r.Op{
case "sign":var seed [32]byte;copy(seed[:],h(r.Seed));pk,sk:=mldsa65.NewKeyFromSeed(&seed);sig:=make([]byte,mldsa65.SignatureSize);e=mldsa65.SignTo(sk,h(r.Msg),h(r.Context),false,sig);b,_:=pk.MarshalBinary();o["pk"]=hex.EncodeToString(b);o["sig"]=hex.EncodeToString(sig)
case "verify":o["valid"]=contract.VerifyCrypto(h(r.PK),h(r.Msg),h(r.Sig));if r.Context!=""{var pk mldsa65.PublicKey;if pk.UnmarshalBinary(h(r.PK))==nil{o["valid"]=mldsa65.Verify(&pk,h(r.Msg),h(r.Context),h(r.Sig))}else{o["valid"]=false}}
case "encode":var b []byte;b,e=contract.EncodeJSON(r.Name,r.API);o["wire"]=hex.EncodeToString(b);o["msg"]=hex.EncodeToString(contract.Frame(r.Domain,b))
case "decode":o["api"],e=contract.Decode(r.Name,h(r.Wire))
case "policy":e=contract.Verify(r.Name,h(r.Wire),h(r.Sig),r.C)
case "decision":o["decision"]=contract.DecideOrder(h(r.Wire),h(r.Sig),r.C,r.Snapshot)
case "snapshot":d:=contract.Decision{Authentication:r.Auth,SnapshotPolicy:contract.PolicyResult{Result:contract.Result{Status:"NOT_RUN"},Source:"SYNTHETIC"},ACK:"NOT_CONNECTED",WALReplay:"NOT_RUN",Ledger:"NOT_CONNECTED"};if r.Snapshot!=nil{d.SnapshotPolicy.SnapshotID=r.Snapshot.ID};if r.Auth.Status=="PASS"{d.SnapshotPolicy=contract.EvaluateSnapshot(r.Snapshot)};o["decision"]=d
case "fee-decimal":o["fee"],e=contract.FeeDecimal(r.Receive,r.Rate)
case "cap":e=contract.CheckCap(r.Cap,r.Rate)
case "fee":o["fee"],e=contract.Fee(r.Receive,r.BPS)
default:e=fmt.Errorf("unsupported")};if e!=nil{o["code"]=e.Error()};return o}
func main(){s:=bufio.NewScanner(os.Stdin);s.Buffer(make([]byte,4096),5<<20);w:=json.NewEncoder(os.Stdout);for s.Scan(){var r R;if e:=json.Unmarshal(s.Bytes(),&r);e!=nil{panic(e)};w.Encode(run(r))};if s.Err()!=nil{panic(s.Err())}}
