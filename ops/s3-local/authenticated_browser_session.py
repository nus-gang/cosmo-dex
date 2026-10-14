"""L-T only: all twelve existing registrations verified BEFORE browser/keys.

The broker belongs to this finite session. No board wait or registration write
is performed while a key-bearing tab exists. Every action has a bounded reply.
"""
from contextlib import contextmanager
import shlex
import approval_gate
import browser_web
import planned_registration
import web_session
from browser_web_cli import parse
from private_reader import broker_at
from runtime_client import RuntimeClient
from web_pid_mailbox import WebMailbox
from workspace_registration import PROJECT


@contextmanager
def session(planned, packet, workspace_id, mailbox, history_factory, stop=lambda:False):
    if type(mailbox) is not WebMailbox or mailbox.used or getattr(mailbox,'_authenticated_attempted',False):
        raise ValueError('BROWSER_SESSION_USED')
    mailbox._authenticated_attempted=True
    if packet not in planned['packets']:raise ValueError('BROWSER_PACKET')
    command,=packet['body']['runtimeConfig']['workspaceRuntime']['commands']
    words=shlex.split(command['command']);argv=words[5:]
    a,_,endpoint,_,_,_,decision,revisions=parse(words[4:])
    if mailbox.root!=a.pid_mailbox:raise ValueError('BROWSER_MAILBOX')
    client=RuntimeClient.from_environment(workspace_id,command['id'])
    def inspect():
        if stop():raise ValueError('STOPPED')
        return approval_gate.inspect(a.bundle,a.artifacts,a.runtime_pin,a.local_demo_profile,True,decision,revisions)
    before=inspect();browser_web.assets(a)
    # Fresh authenticated listing, all twelve old service histories retained.
    planned_registration.board_ready(planned,client.read_workspaces(f'/api/projects/{PROJECT}/workspaces'),history_factory)
    def audit():
        if inspect()!=before:raise ValueError('APPROVAL_CHANGED')
    history=history_factory(packet)
    with web_session.session(words[1],packet['body']['cwd'],argv,fee_bps=int(command['id'].split('fee')[1]),
        mailbox=mailbox,broker_root=endpoint.parent,broker=broker_at,client=client,audit=audit,
        key_packet=packet,**({} if history is None else dict(reentry=history,
        final_authorize=approval_gate.final_authorizer(before,stop)))) as evidence:
        yield evidence
