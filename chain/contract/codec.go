// Package contract implements the S0 v1 strict wire and API codec, not Cosmos TX execution.
package contract

import (
	"bytes"
	_ "embed"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"math/big"
	"regexp"
	"strconv"
)

type Code string

func (c Code) Error() string { return string(c) }

const (
	NonCanonical  Code = "NON_CANONICAL_WIRE"
	IntegerRange  Code = "INTEGER_RANGE"
	ResourceLimit Code = "RESOURCE_LIMIT"
)

type field struct {
	Tag      uint64
	Name     string
	Type     string
	Repeated bool
}

//go:embed schema.json
var schemaBytes []byte
var schema map[string][]field

func init() {
	if err := json.Unmarshal(schemaBytes, &schema); err != nil {
		panic(err)
	}
}

var decimal = regexp.MustCompile(`^(0|[1-9][0-9]*)$`)
var identifier = regexp.MustCompile(`^[A-Za-z0-9._:/-]+$`)

func Integer(s string, bits int) (*big.Int, error) {
	if bits != 32 && bits != 64 && bits != 128 && bits != 256 {
		return nil, IntegerRange
	}
	if len(s) > 78 || !decimal.MatchString(s) {
		return nil, IntegerRange
	}
	n, ok := new(big.Int).SetString(s, 10)
	if !ok || n.BitLen() > bits {
		return nil, IntegerRange
	}
	return n, nil
}
func Atoms(s string) ([]byte, error) {
	n, e := Integer(s, 128)
	if e != nil {
		return nil, e
	}
	return n.FillBytes(make([]byte, 16)), nil
}
func AtomsDecimal(b []byte) (string, error) {
	if len(b) != 16 {
		return "", NonCanonical
	}
	return new(big.Int).SetBytes(b).String(), nil
}
func limit(name string) int {
	switch name {
	case "OrderV1":
		return 8192
	case "CancelV1", "WalletChallengeV1":
		return 1024
	default:
		return 1048576
	}
}
func vi(b []byte, pos *int) (uint64, error) {
	start := *pos
	n, k := binary.Uvarint(b[start:])
	if k <= 0 {
		return 0, NonCanonical
	}
	*pos += k
	if k > 1 && b[*pos-1] == 0 {
		return 0, NonCanonical
	}
	return n, nil
}
func fixed(t string) int {
	return map[string]int{"h": 32, "a": 20, "pk": 1952, "sig": 3309, "atoms": 16}[t]
}

// Decode checks limits, minimal varints, tag order/presence and all nested shapes.
// Values are returned in the canonical API representation (decimal, hex, base64).
func Decode(name string, b []byte) (map[string]any, error) { return decode(name, b, 1) }
func decode(name string, b []byte, depth int) (map[string]any, error) {
	if depth > 4 || len(b) > limit(name) {
		return nil, ResourceLimit
	}
	fs, ok := schema[name]
	if !ok {
		return nil, NonCanonical
	}
	out := map[string]any{}
	counts := map[uint64]int{}
	last := uint64(0)
	pos := 0
	for _, f := range fs {
		if f.Repeated {
			out[f.Name] = []any{}
		}
	}
	for pos < len(b) {
		key, e := vi(b, &pos)
		if e != nil {
			return nil, e
		}
		tag := key >> 3
		wire := key & 7
		var f field
		found := false
		for _, candidate := range fs {
			if candidate.Tag == tag {
				f = candidate
				found = true
				break
			}
		}
		if !found || tag < last || (counts[tag] > 0 && !f.Repeated) {
			return nil, NonCanonical
		}
		last = tag
		counts[tag]++
		if (f.Name == "new_signed_orders" && counts[tag] > 100) || (f.Name == "fills" && counts[tag] > 1000) {
			return nil, ResourceLimit
		}
		var value any
		if f.Type == "u32" || f.Type == "u64" {
			if wire != 0 {
				return nil, NonCanonical
			}
			n, e := vi(b, &pos)
			if e != nil {
				return nil, e
			}
			if f.Type == "u32" && n > 1<<32-1 {
				return nil, IntegerRange
			}
			value = strconv.FormatUint(n, 10)
		} else {
			if wire != 2 {
				return nil, NonCanonical
			}
			n, e := vi(b, &pos)
			if e != nil || n > uint64(len(b)-pos) {
				return nil, NonCanonical
			}
			raw := b[pos : pos+int(n)]
			pos += int(n)
			if _, nested := schema[f.Type]; nested {
				value, e = decode(f.Type, raw, depth+1)
				if e != nil {
					return nil, e
				}
			} else if size := fixed(f.Type); size > 0 {
				if len(raw) != size {
					return nil, NonCanonical
				}
				switch f.Type {
				case "h":
					value = hex.EncodeToString(raw)
				case "atoms":
					value = new(big.Int).SetBytes(raw).String()
				default:
					value = base64.StdEncoding.EncodeToString(raw)
				}
			} else {
				if !validString(f.Name, string(raw)) {
					return nil, NonCanonical
				}
				value = string(raw)
			}
		}
		if f.Repeated {
			out[f.Name] = append(out[f.Name].([]any), value)
		} else {
			out[f.Name] = value
		}
	}
	for _, f := range fs {
		if !f.Repeated && counts[f.Tag] != 1 {
			return nil, NonCanonical
		}
	}
	return out, nil
}
func validString(name, s string) bool {
	if name == "server_origin" {
		for _, c := range []byte(s) {
			if c > 127 || c < 33 {
				return false
			}
		}
		return len(s) > 0 && len(s) <= 255
	}
	return len(s) > 0 && len(s) <= 128 && identifier.MatchString(s)
}
func Encode(name string, m map[string]any) ([]byte, error) { return encode(name, m, 1) }
func encode(name string, m map[string]any, depth int) ([]byte, error) {
	if depth > 4 {
		return nil, ResourceLimit
	}
	fs, ok := schema[name]
	if !ok || len(m) != len(fs) {
		return nil, NonCanonical
	}
	var b []byte
	for _, f := range fs {
		v, exists := m[f.Name]
		if !exists {
			return nil, NonCanonical
		}
		values := []any{v}
		if f.Repeated {
			var ok bool
			values, ok = v.([]any)
			if !ok {
				return nil, NonCanonical
			}
			if (f.Name == "fills" && len(values) > 1000) || (f.Name == "new_signed_orders" && len(values) > 100) {
				return nil, ResourceLimit
			}
		}
		for _, v := range values {
			if f.Type == "u32" || f.Type == "u64" {
				s, ok := v.(string)
				if !ok {
					return nil, IntegerRange
				}
				bits := 64
				if f.Type == "u32" {
					bits = 32
				}
				n, e := Integer(s, bits)
				if e != nil {
					return nil, e
				}
				b = binary.AppendUvarint(b, f.Tag<<3)
				b = binary.AppendUvarint(b, n.Uint64())
			} else {
				var raw []byte
				var e error
				if _, nested := schema[f.Type]; nested {
					o, ok := v.(map[string]any)
					if !ok {
						return nil, NonCanonical
					}
					raw, e = encode(f.Type, o, depth+1)
				} else {
					s, ok := v.(string)
					if !ok {
						if f.Type == "atoms" {
							return nil, IntegerRange
						}
						return nil, NonCanonical
					}
					switch f.Type {
					case "atoms":
						raw, e = Atoms(s)
					case "h":
						raw, e = hex.DecodeString(s)
						if hex.EncodeToString(raw) != s {
							return nil, NonCanonical
						}
					case "a", "pk", "sig":
						raw, e = base64.StdEncoding.Strict().DecodeString(s)
						if base64.StdEncoding.EncodeToString(raw) != s {
							return nil, NonCanonical
						}
					default:
						if !validString(f.Name, s) {
							return nil, NonCanonical
						}
						raw = []byte(s)
					}
					if size := fixed(f.Type); e == nil && size > 0 && len(raw) != size {
						return nil, NonCanonical
					}
				}
				if e != nil {
					return nil, e
				}
				b = binary.AppendUvarint(b, f.Tag<<3|2)
				b = binary.AppendUvarint(b, uint64(len(raw)))
				b = append(b, raw...)
			}
			if len(b) > limit(name) {
				return nil, ResourceLimit
			}
		}
	}
	return b, nil
}

// EncodeJSON rejects duplicate keys before converting JSON to the schema.
func EncodeJSON(name string, raw []byte) ([]byte, error) {
	if len(raw) > 4*limit(name) {
		return nil, ResourceLimit
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	v, e := jsonValue(d, 0)
	if e != nil {
		return nil, e
	}
	if _, e = d.Token(); !errors.Is(e, io.EOF) {
		return nil, NonCanonical
	}
	m, ok := v.(map[string]any)
	if !ok {
		return nil, NonCanonical
	}
	return Encode(name, m)
}
func jsonValue(d *json.Decoder, depth int) (any, error) {
	if depth > 10 {
		return nil, ResourceLimit
	}
	t, e := d.Token()
	if e != nil {
		return nil, NonCanonical
	}
	delim, ok := t.(json.Delim)
	if !ok {
		return t, nil
	}
	switch delim {
	case '{':
		m := map[string]any{}
		for d.More() {
			k, e := d.Token()
			if e != nil {
				return nil, NonCanonical
			}
			s, ok := k.(string)
			if !ok {
				return nil, NonCanonical
			}
			if _, exists := m[s]; exists {
				return nil, NonCanonical
			}
			v, e := jsonValue(d, depth+1)
			if e != nil {
				return nil, e
			}
			m[s] = v
		}
		end, e := d.Token()
		if e != nil || end != json.Delim('}') {
			return nil, NonCanonical
		}
		return m, nil
	case '[':
		a := []any{}
		for d.More() {
			v, e := jsonValue(d, depth+1)
			if e != nil {
				return nil, e
			}
			a = append(a, v)
		}
		end, e := d.Token()
		if e != nil || end != json.Delim(']') {
			return nil, NonCanonical
		}
		return a, nil
	}
	return nil, NonCanonical
}
