//go:build dev_local_demo

package main

import "testing"

func validArgs() []string {
	return []string{"create", "--source-input", "/private/source.json",
		"--effective-profile", "/private/profile.json", "--runtime-pin", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"--user-public-keys", "/private/users.json", "--output", "/private/fee0", "--scratch", "/private/scratch",
		"--run-uuid", "11111111-2222-4333-8444-555555555555", "--genesis-time", "2027-01-15T08:00:00Z",
		"--fee-bps", "0", "--c-validator", "/private/bin/nus-s3-local-demo",
		"--publication-gate", "stdin", "--local-demo-profile", "s3-dev-local/1", "--acknowledge-unproven-space"}
}

func TestParseExactInitializerEnvelope(t *testing.T) {
	for _, fee := range []string{"0", "25"} {
		args := validArgs()
		args[index(args, "--fee-bps")+1] = fee
		got, err := parse(args)
		if err != nil || got.fee != fee || !got.ack || got.pin != "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" {
			t.Fatal("valid initializer envelope rejected")
		}
	}
	base := validArgs()
	cases := [][]string{nil, base[1:], append(append([]string{}, base...), "--fee-bps", "25")}
	for option, value := range map[string]string{
		"--source-input": "relative", "--runtime-pin": "a", "--fee-bps": "025",
		"--local-demo-profile": "s3-dev-local-v1", "--output": "/private/../other",
	} {
		args := append([]string{}, base...)
		args[index(args, option)+1] = value
		cases = append(cases, args)
	}
	args := append([]string{}, base...)
	args = append(args[:index(args, "--acknowledge-unproven-space")], args[index(args, "--acknowledge-unproven-space")+1:]...)
	cases = append(cases, args)
	for _, args := range cases {
		if _, err := parse(args); err == nil {
			t.Fatal("invalid initializer envelope accepted")
		}
	}
}

func index(args []string, value string) int {
	for i, current := range args {
		if current == value {
			return i
		}
	}
	return -1
}
