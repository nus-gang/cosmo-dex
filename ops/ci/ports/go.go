// SRE adapter only: invokes fixed C library; synthetic policy is not authentication.
package main
import ("encoding/json";"encoding/hex";"encoding/base64";"strconv";"fmt";"os"; c "github.com/nus-gang/cosmo-dex/chain/contract";"github.com/cloudflare/circl/sign/mldsa/mldsa65")
func main(){
 var requests []map[string]json.RawMessage;json.NewDecoder(os.Stdin).Decode(&requests)
 out:=[]map[string]any{}
 for _,r:=range requests{
 s:=func(k string)string{var v string;json.Unmarshal(r[k],&v);return v}
 b:=func(k string)[]byte{v,_:=hex.DecodeString(s(k));return v}
 var got any; var e error
 switch s("op"){
 case "rc4":
  var n map[string]json.RawMessage;json.Unmarshal(r["native"],&n)
  var ctx map[string]any;json.Unmarshal(n["context"],&ctx)
  reg:=ctx["registered"].(map[string]any);pk,_:=hex.DecodeString(reg["raw_key_hex"].(string))
  mapped:=map[string]any{"ChainID":ctx["chain_id"],"GenesisHash":ctx["genesis_hash"],"ModuleID":ctx["exchange_module_id"],"MarketID":ctx["market_id"],"MarketConfigVersion":ctx["market_config_version"],"SnapshotID":ctx["snapshot_id"],"RegisteredKey":base64.StdEncoding.EncodeToString(pk)}
  if t,ok:=reg["key_type"];ok{mapped["RegisteredKeyType"]=t}
  for src,dst:=range map[string]string{"height":"Height","epoch":"Epoch"}{if v,ok:=ctx[src];ok&&v!=nil{u,err:=strconv.ParseUint(v.(string),10,64);if err!=nil{panic(err)};mapped[dst]=u}}
  raw,_:=json.Marshal(mapped);var context c.Context;if err:=json.Unmarshal(raw,&context);err!=nil{panic(err)}
  var snap *c.Snapshot;json.Unmarshal(n["snapshot"],&snap)
  var wire,sig string;json.Unmarshal(n["wire_hex"],&wire);json.Unmarshal(n["signature_hex"],&sig);wb,_:=hex.DecodeString(wire);sb,_:=hex.DecodeString(sig)
  got=c.DecideOrder(wb,sb,context,snap)
 case "crypto":got=c.VerifyCrypto(b("pk"),b("input"),b("signature"));if len(b("context"))>0{var pk mldsa65.PublicKey;got=false;if pk.UnmarshalBinary(b("pk"))==nil{got=mldsa65.Verify(&pk,b("input"),b("context"),b("signature"))}}
 case "frame":got=hex.EncodeToString(c.Frame(s("domain"),b("wire")))
 case "atoms":var x []byte;x,e=c.Atoms(s("api"));got=hex.EncodeToString(x)
 case "atoms_decode":got,e=c.AtomsDecimal(b("wire"))
 case "encode":var x []byte;x,e=c.EncodeJSON(s("message"),r["api"]);got=hex.EncodeToString(x)
 case "decode":_,e=c.Decode(s("message"),b("wire"));got="CANONICAL"
 case "fee":got,e=c.FeeDecimal(s("receive"),s("rate"))
 case "cap":e=c.CheckCap(s("cap"),s("rate"));got="OK"
 case "decision":
  var auth c.Result;var snap *c.Snapshot;json.Unmarshal(r["auth"],&auth);json.Unmarshal(r["snapshot"],&snap)
  d:=c.Decision{Authentication:auth,SnapshotPolicy:c.PolicyResult{Result:c.Result{Status:"NOT_RUN"},Source:"SYNTHETIC"},ACK:"NOT_CONNECTED",WALReplay:"NOT_RUN",Ledger:"NOT_CONNECTED"}
  if snap!=nil{d.SnapshotPolicy.SnapshotID=snap.ID};if auth.Status=="PASS"{d.SnapshotPolicy=c.EvaluateSnapshot(snap)};got=d
 default:panic("unsupported op")
 }
 if e!=nil{got=e.Error()};out=append(out,map[string]any{"id":s("id"),"actual":got})
 };v,_:=json.Marshal(out);fmt.Println(string(v))
}
