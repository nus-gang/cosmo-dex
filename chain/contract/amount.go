package contract

import "math/big"

// Arithmetic consumes canonical U128 operands. Legacy vector diagnostics remain
// distinct here; API callers may map FORMAT/RANGE/overflow to INTEGER_RANGE.
func Arithmetic(op, a, b, c string) (string, error) {
	vals := []string{a, b, c}
	ns := make([]*big.Int, 3)
	for i, s := range vals {
		if !decimal.MatchString(s) {
			return "", Code("FORMAT")
		}
		n, e := Integer(s, 128)
		if e != nil {
			return "", Code("RANGE")
		}
		ns[i] = n
	}
	n := new(big.Int)
	switch op {
	case "mul":
		n.Mul(ns[0], ns[1])
		n.Mul(n, ns[2])
		if n.BitLen() > 256 {
			return "", Code("INTERMEDIATE_U256_OVERFLOW")
		}
	case "add":
		n.Add(ns[0], ns[1])
	case "sub":
		n.Sub(ns[0], ns[1])
		if n.Sign() < 0 {
			return "", Code("UNDERFLOW")
		}
	case "fee":
		if !ns[1].IsUint64() || ns[1].Uint64() > 10000 {
			return "", Code("BPS_RANGE")
		}
		return Fee(a, ns[1].Uint64())
	default:
		return "", Code("UNSUPPORTED_OPERATION")
	}
	if n.BitLen() > 128 {
		return "", Code("FINAL_U128_OVERFLOW")
	}
	return n.String(), nil
}

// DevFill is a pure S0 DEV profile calculation, not a ledger transition.
func DevFill(q, p, bps string) (map[string]string, error) {
	qty, e := Integer(q, 64)
	if e != nil {
		return nil, e
	}
	price, e := Integer(p, 64)
	if e != nil {
		return nil, e
	}
	fee, e := Integer(bps, 32)
	if e != nil {
		return nil, e
	}
	if qty.Sign() == 0 || price.Sign() == 0 || qty.Uint64() > 1000000 || price.Uint64() > 1000000 {
		return nil, Code("MARKET_LIMIT")
	}
	base, e := CheckedArithmetic(q, "1000", true)
	if e != nil {
		return nil, e
	}
	quote, e := CheckedArithmetic(q, p, true)
	if e != nil {
		return nil, e
	}
	fb, e := Fee(base, fee.Uint64())
	if e != nil {
		return nil, e
	}
	fq, e := Fee(quote, fee.Uint64())
	if e != nil {
		return nil, e
	}
	return map[string]string{"base": base, "quote": quote, "fee_base": fb, "fee_quote": fq}, nil
}
