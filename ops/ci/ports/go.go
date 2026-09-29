// SRE adapter only: invokes fixed C library; synthetic policy is not authentication.
package main
import ("encoding/json";"encoding/hex";"fmt";"os"; c "github.com/nus-gang/cosmo-dex/chain/contract";"github.com/cloudflare/circl/sign/mldsa/mldsa65")
func main(){
 var requests []map[string]json.RawMessage;json.NewDecoder(os.Stdin).Decode(&requests)
 out:=[]map[string]any{}
 for _,r:=range requests{
 s:=func(k string)string{var v string;json.Unmarshal(r[k],&v);return v}
 b:=func(k string)[]byte{v,_:=hex.DecodeString(s(k));return v}
 var got any; var e error
 switch s("op"){
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
