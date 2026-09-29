package contract

import "strconv"

// Snapshot is an explicitly synthetic, immutable policy input. Pointers preserve
// missing/null state instead of silently manufacturing false or zero values.
type Snapshot struct {
	ID                 *string `json:"id"`
	Source             *string `json:"source"`
	Height             *string `json:"height"`
	Expiry             *string `json:"expiry_height"`
	EpochMatches       *bool   `json:"epoch_matches"`
	Revoked            *bool   `json:"revoked"`
	IDState            *string `json:"id_state"`
	CumulativeOK       *bool   `json:"cumulative_ok"`
	ConfirmedBalanceOK *bool   `json:"confirmed_balance_ok"`
	Q                  *string `json:"q"`
	P                  *string `json:"p"`
	ActiveBPS          *string `json:"active_bps"`
	Cap                *string `json:"cap"`
}
type Result struct {
	Status string  `json:"status"`
	Code   *string `json:"code"`
}
type PolicyResult struct {
	Result
	Source     string  `json:"source"`
	SnapshotID *string `json:"snapshot_id"`
}
type Decision struct {
	Authentication Result       `json:"authentication"`
	SnapshotPolicy PolicyResult `json:"snapshot_policy"`
	ACK            string       `json:"ack"`
	WALReplay      string       `json:"wal_replay"`
	Ledger         string       `json:"ledger"`
}

func result(e error) Result {
	if e == nil {
		s := "OK"
		return Result{"PASS", &s}
	}
	s := e.Error()
	if s == "NOT_CONNECTED" {
		return Result{Status: s}
	}
	return Result{"REJECTED", &s}
}
func activeBPS(s string) (uint64, error) {
	n, e := Integer(s, 32)
	if e != nil || n.Uint64() > 10000 {
		return 0, Code("BPS_RANGE")
	}
	return n.Uint64(), nil
}

// FeeDecimal validates receive before rate, preserving rc3 error priority.
func FeeDecimal(receive, rate string) (string, error) {
	if _, e := Integer(receive, 128); e != nil {
		return "", e
	}
	b, e := activeBPS(rate)
	if e != nil {
		return "", e
	}
	return Fee(receive, b)
}
func CheckCap(cap, rate string) error {
	c, e := Integer(cap, 32)
	if e != nil {
		return e
	}
	b, e := activeBPS(rate)
	if e != nil {
		return e
	}
	if b > c.Uint64() {
		return Code("FEE_CAP")
	}
	return nil
}
func (s *Snapshot) connected() bool {
	return s != nil && s.ID != nil && *s.ID != "" && s.Source != nil && *s.Source == "SYNTHETIC" && s.Height != nil && s.Expiry != nil && s.EpochMatches != nil && s.Revoked != nil && s.IDState != nil && (*s.IDState == "NEW" || *s.IDState == "CONFLICT") && s.CumulativeOK != nil && s.ConfirmedBalanceOK != nil && s.Q != nil && s.P != nil && s.ActiveBPS != nil && s.Cap != nil
}
func snapshotError(s *Snapshot) error {
	for _, v := range []*string{s.Height, s.Expiry, s.Q, s.P} {
		if _, e := Integer(*v, 64); e != nil {
			return e
		}
	}
	if _, e := Integer(*s.Cap, 32); e != nil {
		return e
	}
	if *s.IDState == "CONFLICT" {
		return Code("ID_CONFLICT")
	}
	if !*s.EpochMatches {
		return Code("EPOCH_MISMATCH")
	}
	if *s.Revoked {
		return Code("ORDER_REVOKED")
	}
	h, _ := strconv.ParseUint(*s.Height, 10, 64)
	exp, _ := strconv.ParseUint(*s.Expiry, 10, 64)
	if e := Expiry(h, exp); e != nil {
		return e
	}
	q, _ := strconv.ParseUint(*s.Q, 10, 64)
	p, _ := strconv.ParseUint(*s.P, 10, 64)
	if q == 0 || p == 0 || q > 1000000 || p > 1000000 {
		return Code("MARKET_LIMIT")
	}
	if e := CheckCap(*s.Cap, *s.ActiveBPS); e != nil {
		return e
	}
	if _, e := DevFill(*s.Q, *s.P, *s.ActiveBPS); e != nil {
		return e
	}
	if !*s.CumulativeOK {
		return Code("CUMULATIVE_QTY_EXCEEDED")
	}
	if !*s.ConfirmedBalanceOK {
		return Code("INSUFFICIENT_CONFIRMED_BALANCE")
	}
	return nil
}

// EvaluateSnapshot never authenticates an order or admits it to a ledger.
func EvaluateSnapshot(s *Snapshot) PolicyResult {
	r := PolicyResult{Result: Result{Status: "NOT_CONNECTED"}, Source: "SYNTHETIC"}
	if s != nil {
		r.SnapshotID = s.ID
	}
	if s.connected() {
		r.Result = result(snapshotError(s))
	}
	return r
}

// AuthenticateOrder executes canonical/context/key binding and actual ML-DSA.
// Epoch, expiry and financial policy belong to EvaluateSnapshot.
func AuthenticateOrder(body, sig []byte, c Context) Result {
	return result(verify("OrderV1", body, sig, c, true))
}

// DecideOrder binds the synthetic policy to the authenticated signed fields and
// the caller's snapshot height. It cannot return an ACK from policy success.
func DecideOrder(body, sig []byte, c Context, s *Snapshot) Decision {
	d := Decision{Authentication: AuthenticateOrder(body, sig, c), SnapshotPolicy: PolicyResult{Result: Result{Status: "NOT_RUN"}, Source: "SYNTHETIC"}, ACK: "NOT_CONNECTED", WALReplay: "NOT_RUN", Ledger: "NOT_CONNECTED"}
	if s != nil {
		d.SnapshotPolicy.SnapshotID = s.ID
	}
	if d.Authentication.Status != "PASS" {
		return d
	}
	d.SnapshotPolicy = EvaluateSnapshot(s)
	if !s.connected() {
		return d
	}
	m, _ := Decode("OrderV1", body)
	if c.SnapshotID == "" {
		d.SnapshotPolicy.Result = Result{Status: "NOT_CONNECTED"}
		return d
	}
	if *s.ID != c.SnapshotID || *s.Height != strconv.FormatUint(c.Height, 10) || *s.Expiry != m["expiry_height"] || *s.Q != m["max_qty_lots"] || *s.P != m["limit_price_ticks"] || *s.Cap != m["max_fee_bps"] || *s.EpochMatches != (number(m, "owner_epoch") == c.Epoch) {
		d.SnapshotPolicy.Result = result(Code("CONTEXT_MISMATCH"))
	}
	if d.SnapshotPolicy.Status == "PASS" && (number(m, "side") < 1 || number(m, "side") > 2 || number(m, "order_type") < 1 || number(m, "order_type") > 2 || m["fee_asset_policy_id"] != "RECEIVE_ASSET_V1") {
		d.SnapshotPolicy.Result = result(Code("MARKET_LIMIT"))
	}
	return d
}
