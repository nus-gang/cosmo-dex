//go:build dev_local_demo

package main

import (
	"encoding/json"
	"errors"
	"io"
	"reflect"
)

// Each entry is the exact preflight argv for one validator. No command runs.
func topologyOptions(raw []byte) ([]options, error) {
	reject := errors.New("TOPOLOGY_ARGUMENTS_REJECTED")
	if len(raw) > 65536 || uniqueJSON(raw) != nil {
		return nil, reject
	}
	var argv [][]string
	if json.Unmarshal(raw, &argv) != nil || len(argv) != 4 {
		return nil, reject
	}
	opts := make([]options, 4)
	for i, a := range argv {
		o, e := parse(a)
		if e != nil || o.mode != "preflight" {
			return nil, reject
		}
		if _, e = config(o); e != nil {
			return nil, reject
		}
		if i > 0 && (o.bundle != opts[0].bundle || o.profile != opts[0].profile || o.pin != opts[0].pin) {
			return nil, reject
		}
		opts[i] = o
	}
	return opts, nil
}

func runTopology(args []string, out io.Writer) error {
	reject := errors.New("TOPOLOGY_REJECTED")
	if len(args) != 2 || args[0] != "--topology" {
		return reject
	}
	raw, e := readFile(args[1], 65536, false)
	if e != nil {
		return reject
	}
	opts, e := topologyOptions(raw)
	if e != nil {
		return reject
	}
	in, e := inputs(opts[0]) // reviewed B semantic validation, no store/node creation
	if e != nil {
		return reject
	}
	ids, e := inspectHomeTopology(opts, in)
	if e != nil {
		return reject
	}
	// Detect candidate input changes during the identity observation.
	after, e := inputs(opts[0])
	if e != nil || !reflect.DeepEqual(in, after) {
		return reject
	}
	check, e := readFile(args[1], 65536, false)
	if e != nil || string(raw) != string(check) {
		return reject
	}
	return json.NewEncoder(out).Encode(struct {
		NodeIDs  []string `json:"node_ids"`
		Approval bool     `json:"approval_verified"`
		Ports    bool     `json:"port_availability_verified"`
		Writer   bool     `json:"writer_exclusion_verified"`
	}{NodeIDs: ids})
}
