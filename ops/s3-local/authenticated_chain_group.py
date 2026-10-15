"""L-T four-node entry: full B topology preflight then coordinated STOP.

All four packet targets are fixed before START. Node IDs come from the newly
initialized public topology; the real B checker matches every private home.
"""
from contextlib import contextmanager
import shlex
import approval_gate
import chain_group
import chain_stage
import chain_topology
import chain_topology_check
import authenticated_chain_session
from chain_cli import parse,resolve_peers


@contextmanager
def session(packets,workspace_ids,mailboxes,reentries,evidence_root,stop=lambda:False):
    if any(len(x)!=4 for x in (packets,workspace_ids,mailboxes,reentries)) or len(set(workspace_ids))!=4:
        raise ValueError('CHAIN_GROUP_TARGET')
    words=[shlex.split(p['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command']) for p in packets]
    python=words[0][1];candidate=packets[0]['body']['cwd']
    if any(w[1]!=python or p['body']['cwd']!=candidate for p,w in zip(packets,words)):raise ValueError('CHAIN_GROUP_TARGET')
    args=[parse(w[4:]) for w in words];ids=resolve_peers(args[0])
    if ids is None:raise ValueError('CHAIN_GROUP_TOPOLOGY')
    fee=0 if args[0].p2p=='127.0.0.1:26656' else 25
    nodes=[dict(node_id=ids[i],argv=w[5:]) for i,w in enumerate(words)]
    topology=chain_topology.prepare(python,candidate,nodes,fee_bps=fee)
    if topology['packets']!=packets:raise ValueError('CHAIN_GROUP_TARGET')
    a=args[0]
    before=approval_gate.inspect(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,a.decision,a.revisions)
    with chain_stage.stage(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,a.decision,
            a.revisions,a.input_set.parent,a.input_set.name,a.effective_profile,a.scratch) as staged:
        chain_topology_check.check(python,candidate,nodes,topology,staged,fee_bps=fee,scratch=a.scratch,stopped=stop)
    if stop() or approval_gate.inspect(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,a.decision,a.revisions)!=before:
        raise ValueError('CHAIN_GROUP_APPROVAL')
    with chain_group.group(evidence_root,packets,stop) as (stack,coordinator):
        for i,w in enumerate(words):
            stack.enter_context(authenticated_chain_session.session(python,candidate,w[5:],workspace_ids[i],
                fee_bps=fee,validator_index=i,mailbox=mailboxes[i],stop=stop,reentry=reentries[i],coordinator=coordinator))
        yield coordinator.report
