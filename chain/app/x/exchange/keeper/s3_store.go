package keeper

import (
	"bytes"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math/big"
	"sort"
	"strconv"

	storetypes "github.com/cosmos/cosmos-sdk/store/v2/types"
	sdk "github.com/cosmos/cosmos-sdk/types"
	authtypes "github.com/cosmos/cosmos-sdk/x/auth/types"
	"github.com/nus-gang/cosmo-dex/chain/contract"
)

type S3Config struct {
	Operator   string
	Epoch      uint64
	Admin      string
	FeeBPS     uint64
	FeeVersion uint64
}
type LastBatch struct {
	Seq  uint64
	Hash string
}
type AssetTotal struct {
	Confirmed  string
	Treasury   string
	Unassigned string
}
type OrderRecord struct {
	Hash      string `json:"order_hash"`
	Wire      []byte `json:"order_wire"`
	Signature []byte `json:"signature"`
	Filled    string `json:"settled_qty_lots"`
	Revoked   bool   `json:"revoked"`
}
type BatchIdentity struct {
	Epoch    string   `json:"operator_epoch"`
	Seq      string   `json:"batch_seq"`
	ID       string   `json:"batch_id"`
	Hash     string   `json:"batch_hash"`
	Previous string   `json:"previous_batch_hash"`
	FillIDs  []string `json:"fill_ids"`
}
type S3Receipt struct {
	Context      map[string]string `json:"context"`
	Batch        BatchIdentity     `json:"batch"`
	Disposition  string            `json:"disposition"`
	Height       string            `json:"terminal_height"`
	TxHash       string            `json:"terminal_tx_hash"`
	Wire         []byte            `json:"batch_receipt_v2"`
	FailedTxHash *string           `json:"failed_tx_hash"`
	EvidenceHash *string           `json:"resolution_evidence_hash"`
}

func (k Keeper) S3Key(key string) []byte {
	return []byte("s3/" + hex.EncodeToString(k.GenesisHash) + "/" + S3Market + "/" + key)
}
func (k Keeper) S3Get(ctx sdk.Context, key string) []byte {
	return ctx.KVStore(k.Key).Get(k.S3Key(key))
}
func (k Keeper) S3Set(ctx sdk.Context, key string, value []byte) {
	ctx.KVStore(k.Key).Set(k.S3Key(key), value)
}
func (k Keeper) s3Load(ctx sdk.Context, key string, v any) bool {
	raw := k.S3Get(ctx, key)
	if raw == nil {
		return false
	}
	if e := json.Unmarshal(raw, v); e != nil {
		panic(e)
	}
	return true
}
func (k Keeper) s3Put(ctx sdk.Context, key string, v any) {
	raw, e := json.Marshal(v)
	if e != nil {
		panic(e)
	}
	k.S3Set(ctx, key, raw)
}
func (k Keeper) Config(ctx sdk.Context) S3Config {
	var c S3Config
	if !k.s3Load(ctx, "config", &c) {
		panic("missing S3 config")
	}
	return c
}
func (k Keeper) Last(ctx sdk.Context) LastBatch {
	v := LastBatch{Hash: zeroHash()}
	k.s3Load(ctx, "last", &v)
	return v
}
func (k Keeper) Total(ctx sdk.Context, denom string) AssetTotal {
	v := AssetTotal{"0", "0", "0"}
	k.s3Load(ctx, "total/"+denom, &v)
	return v
}
func (k Keeper) Context(ctx sdk.Context) map[string]string { return k.context(k.Config(ctx)) }
func (k Keeper) context(c S3Config) map[string]string {
	config := S3ConfigHash
	if c.FeeBPS == 25 {
		config = S3Fee25ConfigHash
	}
	return map[string]string{"service_schema": "s3/1", "chain_id": S3ChainID, "genesis_hash": hex.EncodeToString(k.GenesisHash), "contract_hash": S3ContractHash, "config_hash": config, "market_id": S3Market, "market_config_version": "1"}
}
func number128(s string) *big.Int {
	n, e := contract.Integer(s, 128)
	if e != nil {
		panic("corrupt S3 amount")
	}
	return n
}
func checked128(n *big.Int) error {
	if n.Sign() < 0 || n.BitLen() > 128 {
		return S3Error("INTEGER_RANGE")
	}
	return nil
}
func addAmount(a, b string) (string, error) {
	n := new(big.Int).Add(number128(a), number128(b))
	return n.String(), checked128(n)
}
func (k Keeper) s3Position(ctx sdk.Context, owner, denom string) Position {
	amount, epoch := "0", "0"
	if b := k.S3Get(ctx, "a/"+denom+"/"+owner); b != nil {
		amount = string(b)
	}
	if b := k.S3Get(ctx, "e/"+owner); b != nil {
		epoch = string(b)
	}
	return Position{amount, epoch}
}
func (k Keeper) S3Epoch(ctx sdk.Context, owner string) uint64 {
	b := k.S3Get(ctx, "e/"+owner)
	if b == nil {
		return 0
	}
	n, e := Uint(string(b))
	if e != nil {
		panic(e)
	}
	return n
}
func (k Keeper) InitS3(ctx sdk.Context, c S3Config, users, operators [][]byte) {
	k.s3Put(ctx, "config", c)
	k.s3Put(ctx, "last", LastBatch{Hash: zeroHash()})
	for _, raw := range users {
		owner, _ := contract.Address(raw)
		k.S3Set(ctx, "user/"+sdk.AccAddress(owner).String(), raw)
	}
	for _, raw := range operators {
		owner, _ := contract.Address(raw)
		k.S3Set(ctx, "operator/"+sdk.AccAddress(owner).String(), raw)
	}
	for _, denom := range []string{Base, Quote} {
		k.s3Put(ctx, "total/"+denom, AssetTotal{"0", "0", "0"})
	}
}

// Aggregates avoid scanning lifetime orders/accounts on every settlement. The
// independent snapshot audit recomputes sum(C) from the fixed registered users.
func (k Keeper) ReconcileAsset(ctx sdk.Context, denom string, quarantine bool) error {
	t := k.Total(ctx, denom)
	expected := new(big.Int).Add(number128(t.Confirmed), number128(t.Treasury))
	expected.Add(expected, number128(t.Unassigned))
	actual := k.Bank.GetBalance(ctx, authtypes.NewModuleAddress(Module), denom).Amount.BigInt()
	d := new(big.Int).Sub(actual, expected)
	if d.Sign() < 0 {
		return S3Error("ASSET_DEFICIT")
	}
	if d.Sign() > 0 && quarantine {
		u := new(big.Int).Add(number128(t.Unassigned), d)
		if e := checked128(u); e != nil {
			return e
		}
		t.Unassigned = u.String()
		k.s3Put(ctx, "total/"+denom, t)
		ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_quarantine", sdk.NewAttribute("denom", denom), sdk.NewAttribute("atoms", d.String())))
	}
	return nil
}
func (k Keeper) s3Invariant(ctx sdk.Context) error {
	for _, d := range []string{Base, Quote} {
		if e := k.ReconcileAsset(ctx, d, false); e != nil {
			return e
		}
	}
	return nil
}
func (k Keeper) s3ChangeTotal(ctx sdk.Context, denom string, n uint64, withdraw bool) error {
	t := k.Total(ctx, denom)
	value := number128(t.Confirmed)
	if withdraw {
		value.Sub(value, new(big.Int).SetUint64(n))
	} else {
		value.Add(value, new(big.Int).SetUint64(n))
	}
	if e := checked128(value); e != nil {
		return e
	}
	t.Confirmed = value.String()
	k.s3Put(ctx, "total/"+denom, t)
	return nil
}
func orderKey(m map[string]any) string {
	return "order/" + hex.EncodeToString(Raw(m, "owner")) + "/" + Text(m, "owner_epoch") + "/" + Text(m, "order_id")
}
func (k Keeper) loadOrder(ctx sdk.Context, key string) (OrderRecord, bool) {
	b := k.S3Get(ctx, key)
	if b == nil {
		return OrderRecord{}, false
	}
	if len(b) < 45 {
		panic("corrupt order")
	}
	n := int(binary.BigEndian.Uint32(b[41:45]))
	if n > len(b)-45 || (len(b)-45-n != 3309 && len(b)-45-n != 0) {
		panic("corrupt order length")
	}
	return OrderRecord{hex.EncodeToString(b[:32]), bytes.Clone(b[45 : 45+n]), bytes.Clone(b[45+n:]), strconv.FormatUint(binary.BigEndian.Uint64(b[32:40]), 10), b[40] == 1}, true
}
func (k Keeper) saveOrder(ctx sdk.Context, key string, r OrderRecord) {
	b, e := hex.DecodeString(r.Hash)
	if e != nil {
		panic(e)
	}
	n, e := Uint(r.Filled)
	if e != nil {
		panic(e)
	}
	b = binary.BigEndian.AppendUint64(b, n)
	flag := byte(0)
	if r.Revoked {
		flag = 1
	}
	b = append(b, flag)
	b = binary.BigEndian.AppendUint32(b, uint32(len(r.Wire)))
	b = append(b, r.Wire...)
	b = append(b, r.Signature...)
	k.S3Set(ctx, key, b)
}
func (k Keeper) Order(ctx sdk.Context, hash string) (*OrderRecord, error) {
	key := k.S3Get(ctx, "order_index/"+hash)
	if key == nil {
		return nil, nil
	}
	r, ok := k.loadOrder(ctx, string(key))
	if !ok || r.Hash != hash {
		return nil, S3Error("RECEIPT_INCONSISTENCY")
	}
	return &r, nil
}

func (k Keeper) StoredReceipt(ctx sdk.Context, seq uint64, last LastBatch) (*S3Receipt, error) {
	if seq == 0 || seq > last.Seq {
		return nil, nil
	}
	key := "receipt/" + strconv.FormatUint(seq, 10)
	raw := k.S3Get(ctx, key)
	digest := k.S3Get(ctx, key+"/digest")
	if raw == nil || !bytes.Equal(digest, []byte(rawHash(raw))) {
		return nil, S3Error("RECEIPT_INCONSISTENCY")
	}
	var r S3Receipt
	if json.Unmarshal(raw, &r) != nil || r.Batch.Seq != strconv.FormatUint(seq, 10) || r.Context["genesis_hash"] != hex.EncodeToString(k.GenesisHash) || r.Context["chain_id"] != S3ChainID || r.Context["market_id"] != S3Market || seq == last.Seq && r.Batch.Hash != last.Hash {
		return nil, S3Error("RECEIPT_INCONSISTENCY")
	}
	if r.Disposition != "COMMITTED" && r.Disposition != "VOID" {
		return nil, S3Error("RECEIPT_INCONSISTENCY")
	}
	return &r, nil
}

func (k Keeper) slot(ctx sdk.Context, b *Batch, c S3Config, close bool) (*S3Receipt, error) {
	last := k.Last(ctx)
	seq := Number(b.Body, "batch_seq")
	if last.Seq > 0 {
		if _, e := k.StoredReceipt(ctx, last.Seq, last); e != nil {
			return nil, e
		}
	}
	if seq <= last.Seq {
		r, e := k.StoredReceipt(ctx, seq, last)
		if e != nil {
			return nil, e
		}
		if r == nil {
			return nil, S3Error("BATCH_SEQUENCE_GAP")
		}
		if r.Batch.ID != Text(b.Body, "batch_id") || r.Batch.Hash != b.Hash {
			return nil, S3Error("BATCH_CONFLICT")
		}
		if r.Disposition == "VOID" && !close {
			return nil, S3Error("BATCH_CLOSED")
		}
		return r, nil
	}
	if last.Seq == ^uint64(0) {
		return nil, S3Error("SEQUENCE_OVERFLOW")
	}
	if seq != last.Seq+1 {
		return nil, S3Error("BATCH_SEQUENCE_GAP")
	}
	if Text(b.Body, "previous_batch_hash") != last.Hash {
		return nil, S3Error("PREVIOUS_BATCH_HASH_MISMATCH")
	}
	if !close && Number(b.Body, "operator_epoch") != c.Epoch {
		return nil, S3Error("OPERATOR_EPOCH_MISMATCH")
	}
	return nil, nil
}
func (k Keeper) terminal(ctx sdk.Context, b *Batch, c S3Config, failed, evidence *string) error {
	ids := make([]string, 0, len(b.Fills))
	for _, v := range b.Fills {
		ids = append(ids, Text(v.(map[string]any), "fill_id"))
	}
	r := S3Receipt{Context: k.context(c), Batch: BatchIdentity{Text(b.Body, "operator_epoch"), Text(b.Body, "batch_seq"), Text(b.Body, "batch_id"), b.Hash, Text(b.Body, "previous_batch_hash"), ids}, Disposition: "COMMITTED", Height: strconv.FormatInt(ctx.BlockHeight(), 10), TxHash: rawHash(ctx.TxBytes()), FailedTxHash: failed, EvidenceHash: evidence}
	if failed != nil {
		r.Disposition = "VOID"
	} else {
		var e error
		r.Wire, e = contract.Encode("BatchReceiptV1", map[string]any{"protocol_version": "2", "chain_id": S3ChainID, "genesis_hash": hex.EncodeToString(k.GenesisHash), "market_id": S3Market, "batch_seq": r.Batch.Seq, "batch_id": r.Batch.ID, "batch_hash": r.Batch.Hash, "committed_height": r.Height, "tx_hash": r.TxHash})
		if e != nil {
			return wireError(e)
		}
	}
	raw, e := json.Marshal(r)
	if e != nil {
		return e
	}
	// The stored receipt is canonical JSON too, not only its ABCI projection.
	var canonical map[string]any
	if e = json.Unmarshal(raw, &canonical); e != nil {
		return e
	}
	raw, e = json.Marshal(canonical)
	if e != nil {
		return e
	}
	if len(raw) > 4096 {
		return S3Error("RESOURCE_LIMIT")
	}
	key := "receipt/" + r.Batch.Seq
	k.S3Set(ctx, key, raw)
	k.S3Set(ctx, key+"/digest", []byte(rawHash(raw)))
	k.s3Put(ctx, "last", LastBatch{Number(b.Body, "batch_seq"), b.Hash})
	k.S3Set(ctx, "terminal/"+r.Height+"/"+fmt.Sprintf("%010d", ctx.TxIndex()), []byte(r.Batch.Seq))
	ctx.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_receipt", sdk.NewAttribute("batch_seq", r.Batch.Seq), sdk.NewAttribute("batch_id", r.Batch.ID), sdk.NewAttribute("batch_hash", r.Batch.Hash), sdk.NewAttribute("disposition", r.Disposition), sdk.NewAttribute("original_tx_hash", r.TxHash)))
	return nil
}
func (k Keeper) S3OwnerEvent(ctx sdk.Context, kind, owner, before, after, denom, amount, id string, order *string) {
	addr, e := sdk.AccAddressFromBech32(owner)
	if e != nil {
		panic(e)
	}
	event := map[string]any{"kind": kind, "owner": []byte(addr), "before_epoch": before, "after_epoch": after, "order_hash": order, "denom": denom, "amount_atoms": amount, "request_id": id, "tx_hash": rawHash(ctx.TxBytes()), "tx_index": strconv.Itoa(ctx.TxIndex())}
	k.s3Put(ctx, "event/"+strconv.FormatInt(ctx.BlockHeight(), 10)+"/"+fmt.Sprintf("%010d", ctx.TxIndex()), event)
}

type KVTrace struct {
	ReadOps    int `json:"read_ops"`
	ReadBytes  int `json:"read_bytes"`
	WriteOps   int `json:"write_ops"`
	WriteBytes int `json:"write_bytes"`
}
type budgetPanic struct{}

func (b *KVTrace) check() {
	if b.ReadOps > 128 || b.ReadBytes > 262144 || b.WriteOps > 96 || b.WriteBytes > 131072 {
		panic(budgetPanic{})
	}
}

type tracedMulti struct {
	storetypes.MultiStore
	trace *KVTrace
}

func (s tracedMulti) GetKVStore(key storetypes.StoreKey) storetypes.KVStore {
	return tracedKV{s.MultiStore.GetKVStore(key), s.trace}
}

type tracedKV struct {
	storetypes.KVStore
	trace *KVTrace
}

func (s tracedKV) Get(key []byte) []byte {
	v := s.KVStore.Get(key)
	s.trace.ReadOps++
	s.trace.ReadBytes += len(key) + len(v)
	s.trace.check()
	return v
}
func (s tracedKV) Has(key []byte) bool {
	v := s.KVStore.Has(key)
	s.trace.ReadOps++
	s.trace.ReadBytes += len(key)
	s.trace.check()
	return v
}
func (s tracedKV) Set(key, value []byte) {
	s.trace.WriteOps++
	s.trace.WriteBytes += len(key) + len(value)
	s.trace.check()
	s.KVStore.Set(key, value)
}
func (s tracedKV) Delete(key []byte) {
	s.trace.WriteOps++
	s.trace.WriteBytes += len(key)
	s.trace.check()
	s.KVStore.Delete(key)
}
func (s tracedKV) Iterator(_, _ []byte) storetypes.Iterator        { panic(budgetPanic{}) }
func (s tracedKV) ReverseIterator(_, _ []byte) storetypes.Iterator { panic(budgetPanic{}) }

func (k Keeper) s3Atomic(ctx sdk.Context, fn func(sdk.Context) error) (err error) {
	defer func() {
		if r := recover(); r != nil {
			if _, ok := r.(budgetPanic); ok {
				err = S3Error("KV_BUDGET_EXCEEDED")
			} else {
				panic(r)
			}
		}
	}()
	cache, write := ctx.CacheContext()
	trace := &KVTrace{}
	cache = cache.WithMultiStore(tracedMulti{cache.MultiStore(), trace})
	if e := fn(cache); e != nil {
		return e
	}
	raw, _ := json.Marshal(trace)
	cache.EventManager().EmitEvent(sdk.NewEvent("exchange_s3_kv", sdk.NewAttribute("trace", string(raw))))
	write()
	return nil
}
func sortedKeys[T any](m map[string]T) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
