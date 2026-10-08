"""Pure four-validator command consistency; no key/home inspection or service IO."""
import re
from chain_cli import parse
from workspace_registration import prepare_chain

ERROR = 'CHAIN_TOPOLOGY_REJECTED'


def prepare(python, candidate, nodes, *, fee_bps):
    """nodes: ordered v0..v3 {node_id, argv}; IDs must later match private homes.

    Lexical path isolation is not filesystem/inode isolation. Port uniqueness is
    not availability. This packet is neither approval nor a launch permit.
    """
    try:
        if type(nodes) not in (list, tuple) or len(nodes) != 4:
            raise ValueError()
        copied = []
        for node in nodes:
            if type(node) is not dict or set(node) != {'node_id', 'argv'}:
                raise ValueError()
            if type(node['node_id']) is not str or re.fullmatch('[0-9a-f]{40}', node['node_id']) is None:
                raise ValueError()
            if type(node['argv']) not in (list, tuple) or any(type(x) is not str for x in node['argv']):
                raise ValueError()
            copied.append((node['node_id'], tuple(node['argv'])))
        if len({n[0] for n in copied}) != 4:
            raise ValueError()
        args = [parse(['serve-chain-reviewed', *argv]) for _, argv in copied]
        common = ('bundle', 'artifacts', 'input_set', 'effective_profile',
                  'runtime_pin', 'local_demo_profile', 'decision', 'revisions',
                  'lifetime_seconds')
        if any(getattr(a, k) != getattr(args[0], k) for a in args for k in common):
            raise ValueError()
        ports = [p for a in args for p in (a.rpc, a.p2p)]
        if len(set(ports)) != 8:
            raise ValueError()
        for i, a in enumerate(args):
            expected = {ident+'@'+args[j].p2p for j, (ident, _) in enumerate(copied) if j != i}
            if set(a.peers.split(',')) != expected:
                raise ValueError()
        # Separate mutable roots across nodes and purposes, including broker roots.
        roots = [p for a in args for p in (a.home, a.scratch, a.pid_mailbox, a.approval_socket.parent)]
        if any(p == q or p in q.parents or q in p.parents
               for i, p in enumerate(roots) for q in roots[i+1:]):
            raise ValueError()
        inputs = (args[0].bundle, args[0].artifacts, args[0].input_set, args[0].effective_profile)
        if any(p == q or p in q.parents or q in p.parents for p in roots for q in inputs):
            raise ValueError()
        packets = [prepare_chain(python, candidate, argv, fee_bps=fee_bps, validator_index=i)
                   for i, (_, argv) in enumerate(copied)]
        return dict(schema='s3-local-chain-topology/1', fee_bps=fee_bps,
                    node_ids=[n[0] for n in copied], endpoints=ports, packets=packets,
                    topology_commands_consistent=True, home_identity_verified=False,
                    ports_available_verified=False, approval_verified=False,
                    starts_service=False)
    except (ValueError, TypeError, KeyError, AttributeError):
        raise ValueError(ERROR) from None


def preflight_payload(python, candidate, nodes, packet, staged, *, fee_bps):
    """Encode Go topology CLI input from the exact registered command candidate.

    Uses the live staged scope, not mutable original input paths. No subprocess,
    home/key read, port probe, registration or service start occurs here.
    """
    import hashlib
    import json
    from chain_preflight import arguments
    try:
        # Copy caller-owned argv before validation and use only that snapshot.
        import copy
        nodes = copy.deepcopy(nodes)
        expected = prepare(python, candidate, nodes, fee_bps=fee_bps)
        if packet != expected:
            raise ValueError()
        parsed = [parse(['serve-chain-reviewed', *n['argv']]) for n in nodes]
        staged.verify()
        from preflight import bounded, checked_root
        input_raw = bounded(checked_root(staged.input_set.parent), staged.input_set.name, 48*1024*1024)
        profile_raw = bounded(checked_root(staged.effective_profile.parent), staged.effective_profile.name, 1024*1024)
        if (hashlib.sha256(input_raw).hexdigest() != staged.capture_sha256 or
                hashlib.sha256(profile_raw).hexdigest() != staged.profile_sha256):
            raise ValueError()
        # Prevent a different otherwise-valid stage from being substituted.
        profile = parsed[0].effective_profile
        if bounded(checked_root(profile.parent), profile.name, 1024*1024) != profile_raw:
            raise ValueError()
        from preflight import verify_input_set
        a = parsed[0]
        raw, _ = verify_input_set(a.bundle, a.artifacts, a.runtime_pin,
            a.local_demo_profile, True, a.input_set.parent, a.input_set.name)
        if raw != input_raw:
            raise ValueError()
        argv = [arguments(staged, a.runtime_pin, str(a.home), a.rpc, a.p2p, a.peers)[1:]
                for a in parsed]
        raw = json.dumps(argv, separators=(',', ':'), ensure_ascii=True).encode()
        if len(raw) > 65536:
            raise ValueError()
        staged.verify()
        return raw
    except (ValueError, TypeError, KeyError, AttributeError, OSError):
        raise ValueError(ERROR) from None
