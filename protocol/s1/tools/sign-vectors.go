// Fixture utility, run from chain with its preserved CIRCL v1.6.3 dependency.
package main
import (
 "bytes"
 "crypto/sha256"
 "encoding/hex"
 "encoding/json"
 "fmt"
 "os"
 "github.com/cloudflare/circl/sign/mldsa/mldsa65"
)
func decode(s string) []byte { b,e:=hex.DecodeString(s); if e!=nil {panic(e)};return b }
func main(){
 p:="../protocol/s1/vectors/direct.json"
 raw,e:=os.ReadFile(p);if e!=nil {panic(e)}
 var v map[string]interface{};if e=json.Unmarshal(raw,&v);e!=nil {panic(e)}
 var seed [32]byte;copy(seed[:],decode(v["test_seed_hex"].(string)))
 pk,sk:=mldsa65.NewKeyFromSeed(&seed);pb,_:=pk.MarshalBinary()
 if !bytes.Equal(pb,decode(v["public_key_hex"].(string))) {panic("pk mismatch")}
 generate:=len(os.Args)>1&&os.Args[1]=="--generate"
 count:=0
 for _,row:=range v["cases"].([]interface{}){
  c:=row.(map[string]interface{});msg:=decode(c["sign_doc_hex"].(string))
  sig:=make([]byte,mldsa65.SignatureSize)
  if e=mldsa65.SignTo(sk,msg,nil,false,sig);e!=nil {panic(e)}
  if generate {c["signature_hex"]=hex.EncodeToString(sig)} else if !bytes.Equal(sig,decode(c["signature_hex"].(string))) {panic("signature mismatch")}
  if !mldsa65.Verify(pk,msg,nil,sig) {panic("verify")};count++
  bad:=append([]byte(nil),msg...);bad[len(bad)-1]^=1
  if mldsa65.Verify(pk,bad,nil,sig) {panic("mutated bytes accepted")};count++
  h:=sha256.Sum256(msg);if mldsa65.Verify(pk,h[:],nil,sig) {panic("prehash accepted")};count++
 }
 if generate {out,_:=json.MarshalIndent(v,"","  ");if e=os.WriteFile(p,append(out,'\n'),0644);e!=nil {panic(e)}}
 fmt.Printf("PASS CIRCL-only ML-DSA: %d checks; SDK ante/actual TX NOT_RUN\n",count)
}
