"""RC8 caller regressions: real clock/filesystem, synthetic control/auth/probes.

No sockets, actual service requests, or clock/threshold changes in the delay
matrix. The synthetic START is only recorded in a list.
"""
import copy
import hashlib
import approval_gate
import review_documents
from test_native_review import approved, uid
from test_review_documents import documents, REVISIONS, SUBJECT
from contextlib import contextmanager
import json
from pathlib import Path
import shlex
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

import chain_session
import managed_session
import web_session
import workspace_reentry as m
from pid_mailbox import Mailbox, _write
from web_pid_mailbox import WebMailbox
from registration_render import render, key_packets
from test_registration_render import fixture
from test_workspace_reentry import synthetic, probe, WID, OID
from workspace_registration import registered


class FreshnessTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(); self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name).resolve(); self.root.chmod(0o700)

    def archive(self, record, name):
        root = self.root/name; root.mkdir(mode=0o700)
        box = root/'mailbox'; box.mkdir(mode=0o700)
        _write(box, 'challenge', record['challenge'].encode())
        _write(box, 'pids.json', record['pid_raw'].encode())
        record.update(schema='s3-local-stopped-receipt/2', archive_root=str(root))
        _write(root, 'receipt.json', m.encode(record))
        return root/'receipt.json'

    def packet(self, kind, fee, control):
        spec, reports = fixture()
        index = 6 if fee else 0
        packet = (key_packets(spec)[int(bool(fee))] if kind == 'keys' else
                  render(spec, reports)['packets'][index+{'worker':0,'web':1,'chain':2}[kind]])
        return json.loads(json.dumps(packet).replace('/private/control', str(control)))

    def case(self, kind, fee, mode):
        root = self.root/(kind+str(fee)+mode); root.mkdir(mode=0o700)
        packet = self.packet(kind, fee, root)
        _, args, endpoints = m.command_info(packet)
        args.pid_mailbox.parent.mkdir(mode=0o700, parents=True)
        args.pid_mailbox.parent.chmod(0o700)
        box_type = WebMailbox if kind in ('keys','web') else Mailbox
        box = box_type(args.pid_mailbox)
        before_bytes = (box.root/'challenge').read_bytes()
        prior = synthetic(packet)
        receipt = self.archive(prior, root.name+'-prior')
        entry = m.Reentry(packet, [receipt])
        current = copy.deepcopy(prior['stop_response']['workspace'])
        after = synthetic(packet, nonce=box.nonce, sid=OID)
        after['stop_response']['workspace']['runtimeServices'].extend(current['runtimeServices'])
        events = []; counts = dict(read=0, audit=0, probe=0)
        rows=documents(); issue=approved()
        subject=approval_gate.bound_subject(SUBJECT,uid(5))
        for row,role in zip(rows,review_documents.AUTHORS):
            row['body']=review_documents.approval_body(subject,role)
        identity=dict(format='s3-local-approval-audit/1',approval_prerequisites_match=True,
            reusable_permit=False,native_decision_id=uid(5),independent_revisions=REVISIONS,
            review_subject_sha256=hashlib.sha256(subject).hexdigest())
        def mutate(boundary):
            if not mode.startswith(boundary+'-'):return
            effect=mode[len(boundary)+1:]
            if effect=='native-revoke':issue['status']='in_progress'
            elif effect=='native-replace':issue['executionState']['lastDecisionId']=uid(99)
            else:
                role,action=effect.split('-');row=rows[int(role=='cto')]
                if action=='revoke':row['body']=row['body'].replace('approved','revoked')
                else:row['latestRevisionId']=uid(99)
        def reader(path):
            event('authority-get')
            if path.endswith(approval_gate.native_review.ISSUE):return copy.deepcopy(issue)
            return copy.deepcopy(next(row for row in rows if path.endswith('/'+row['key'])))
        def authorize():
            event('final-authorize')
            if mode=='slow-authority':time.sleep(2.1)
            if mode=='authority-io':raise OSError()
            if mode=='authority-interrupt':raise KeyboardInterrupt()
            approval_gate._recheck(reader,identity)
            event('final-authorize-done')

        def event(name):
            events.append(dict(event=name, ns=time.monotonic_ns()))

        def read(path):
            counts['read'] += 1; n = counts['read']; event('get'+str(n))
            if n == 3:
                mutate('final')
                if mode == 'slow-final': time.sleep(2.1)
                if mode == 'failed-final': raise OSError('synthetic GET failure')
            result = copy.deepcopy(current if n <= 3 else after['stop_response']['workspace'])
            if mode == 'drift'+str(n): result['runtimeConfig']['desiredState'] = 'running'
            if mode == 'row'+str(n): result['runtimeServices'][0]['status'] = 'running'
            event('get'+str(n)+'-done')
            return [result]

        def audit():
            counts['audit'] += 1; n = counts['audit']; event('audit'+str(n))
            if mode == 'slow-audit'+str(n): time.sleep(2.1)
            if mode in ('revoked'+str(n), 'replaced'+str(n)): raise ValueError('synthetic approval denial')
            if mode == 'interrupt' and n == 2: raise KeyboardInterrupt()
            event('audit'+str(n)+'-done')

        def resources(record):
            counts['probe'] += 1; n = counts['probe']; event('probe'+str(n))
            result = probe(record)
            if n == 2:
                mutate('probe')
                if mode == 'probe-timeout': raise TimeoutError('bounded probe timeout')
                if mode == 'pid-busy': result['process']['listed_pids_absent'] = False
                if mode == 'port-busy': result['port']['simultaneous_bind_verified'] = False
                if mode == 'writer-busy': result['writer'] = {}
            event('probe'+str(n)+'-done')
            return result

        def request(value):
            action = value['path'].rsplit('/',1)[1]; event(action)
            response = copy.deepcopy(after['stop_response'])
            if action == 'start':
                _write(box.root, 'pids.json', after['pid_raw'].encode())
                response['operation']['phase'] = 'workspace_provision'
                response['operation']['metadata']['action'] = 'start'
            return response

        @contextmanager
        def broker(root): yield root/'s'

        command = packet['body']['runtimeConfig']['workspaceRuntime']['commands'][0]
        words = shlex.split(command['command'])
        common = dict(fee_bps=fee, broker_root=args.approval_socket.parent,
                      broker=broker, audit=audit, reentry=entry, final_authorize=None if mode=='missing-authority' else authorize)
        if kind == 'worker':
            entry.bind_mailbox(box)
            ctx = managed_session._session(words[1], packet['body']['cwd'], words[5:],
                read_workspaces=read, request=request, **common)
        else:
            client = Mock(_workspace_id=WID, read_workspaces=read, request=request)
            release = probe(after)
            common.update(mailbox=box, client=client,
                process=lambda _:release['process'], port=lambda _:release['port'])
            if kind == 'chain':
                common.update(validator_index=0, writer=lambda _:dict(release['writer'],
                    schema='s3-local-chain-writer-probe/1', continuous_exclusion_verified=False))
                ctx = chain_session._session(words[1], packet['body']['cwd'], words[5:], **common)
            else:
                if kind == 'keys': common['key_packet'] = packet
                ctx = web_session._session(words[1], packet['body']['cwd'], words[5:], **common)
        success = mode in ('ok','slow-audit1','slow-audit2')
        with patch.object(m, 'fresh_resources', side_effect=resources):
            if success:
                with ctx: pass
            else:
                with self.assertRaises(KeyboardInterrupt if mode in ('interrupt','authority-interrupt') else ValueError):
                    with ctx: self.fail('unexpected START')
        names = [e['event'] for e in events]
        self.assertEqual(names.count('start'), int(success))
        self.assertEqual(names.count('stop'), int(success))
        if success:
            self.assertEqual(counts['audit'], 2); self.assertEqual(counts['probe'], 2)
            self.assertLess(names.index('audit2-done'), names.index('get2'))
            self.assertLess(names.index('probe2-done'), names.index('get3'))
            self.assertLess(names.index('get3-done'), names.index('final-authorize'))
            self.assertLess(names.index('final-authorize-done'), names.index('start'))
            if 'slow-audit' in mode:
                n=mode[-1]; stamps={e['event']:e['ns'] for e in events}
                self.assertGreaterEqual(stamps['audit'+n+'-done']-stamps['audit'+n],2_000_000_000)
            if kind in ('worker','chain'):
                self.assertTrue(all('writer' in rows[0] for rows in entry.observations))
        else:
            self.assertEqual(set(p.name for p in box.root.iterdir()), {'challenge'})
            self.assertEqual((box.root/'challenge').read_bytes(), before_bytes)
            with self.assertRaises(ValueError): box_type(box.root)
            with self.assertRaises(ValueError): m.preserve(box, packet, {})
            prior_counts=counts.copy()
            with self.assertRaises(ValueError): entry.begin()
            self.assertEqual(counts,prior_counts)
        print(json.dumps(dict(kind=kind,fee=fee,mode=mode,synthetic=True,
                              start=int(success),events=events),sort_keys=True),flush=True)

    def test_real_slow_approval_and_final_get_four_roles_both_fees(self):
        for fee in (0,25):
            for kind in ('keys','web','worker','chain'):
                for mode in ('ok','slow-audit1','slow-audit2','slow-final'):
                    with self.subTest(kind=kind,fee=fee,mode=mode): self.case(kind,fee,mode)

    def test_drift_revocation_reoccupation_and_interrupt_start_nothing(self):
        for fee in (0,25):
            for kind in ('keys','web','worker','chain'):
                modes = ['drift2','drift3','row2','row3','failed-final','revoked1','revoked2',
                         'replaced2','interrupt','probe-timeout','pid-busy','port-busy']
                if kind in ('worker','chain'): modes.append('writer-busy')
                for mode in modes:
                    with self.subTest(kind=kind,fee=fee,mode=mode): self.case(kind,fee,mode)

    def test_post_audit_revocation_replacement_and_authority_failure(self):
        for fee in (0,25):
            for kind in ('keys','web','worker','chain'):
                modes=[boundary+'-'+role+'-'+effect for boundary in ('probe','final')
                       for role in ('ceo','cto','native') for effect in ('revoke','replace')]
                modes+=['slow-authority','authority-io','authority-interrupt','missing-authority']
                for mode in modes:
                    with self.subTest(kind=kind,fee=fee,mode=mode):self.case(kind,fee,mode)

    def test_challenge_only_kept_and_explicit_new_candidate_preparation(self):
        for fee in (0,25):
            old = self.packet('keys', fee, self.root/('old'+str(fee)))
            a = m.command_info(old)[1]; a.pid_mailbox.parent.mkdir(mode=0o700,parents=True)
            a.pid_mailbox.parent.chmod(0o700)
            failed = WebMailbox(a.pid_mailbox); original=(failed.root/'challenge').read_bytes()
            record = synthetic(old); receipt=self.archive(record,'stopped'+str(fee))
            spec,_=fixture();spec.update(candidate='/new-candidate',bundle='/new-bundle',artifacts='/new-artifacts',
                runtime_pin='e'*64,native_decision_id='44444444-2222-4333-8444-555555555555',
                ceo_revision='55555555-2222-4333-8444-555555555555',cto_revision='66666666-2222-4333-8444-555555555555',
                control_root=str(self.root/('new'+str(fee))))
            new=key_packets(spec)[int(bool(fee))]; a=m.command_info(new)[1]
            a.pid_mailbox.parent.mkdir(mode=0o700,parents=True);a.pid_mailbox.parent.chmod(0o700)
            fresh=WebMailbox(a.pid_mailbox)
            self.assertNotEqual(fresh.nonce,failed.nonce)
            with self.assertRaises(ValueError): WebMailbox(failed.root)
            with self.assertRaises(FileNotFoundError): m.read_receipt(failed.root/'receipt.json')
            with self.assertRaises(ValueError): m.Reentry(new,[])
            with self.assertRaises(ValueError): m.Reentry(new,[receipt])
            entry=m.Reentry(new,[receipt],transition=dict(previous=old,next=new));entry.bind_mailbox(fresh)
            listing=copy.deepcopy(record['stop_response']['workspace'])
            listing.update(copy.deepcopy(new['body']))
            with patch.object(m,'fresh_resources',side_effect=probe):
                registered(new,[listing],reentry=entry);registered(new,[listing],reentry=entry)
                entry.confirm(new,[listing],WID)
            self.assertEqual((failed.root/'challenge').read_bytes(),original)
            self.assertEqual(set(p.name for p in failed.root.iterdir()),{'challenge'})

    def test_two_prior_candidates_new_preparation_and_final_keep_all_rows(self):
        for fee in (0,25):
            spec,reports=fixture();packets=[];records=[];paths=[]
            for i in range(3):
                spec.update(candidate='/candidate'+str(i),bundle='/bundle'+str(i),artifacts='/artifacts'+str(i),
                    runtime_pin=str(i+1)*64,native_decision_id=f'{i+1:08d}-2222-4333-8444-555555555555',
                    ceo_revision=f'{i+4:08d}-2222-4333-8444-555555555555',
                    cto_revision=f'{i+7:08d}-2222-4333-8444-555555555555',control_root='/private/control'+str(i))
                packet=key_packets(spec)[int(bool(fee))];packets.append(packet)
                record=synthetic(packet,nonce=str(i+1)*64,sid=f'{i+1:08d}-3333-4333-8333-333333333333')
                records.append(record);paths.append(self.archive(record,f'history{fee}-{i}'))
            for report in reports.values():report['runtime_pin']=spec['runtime_pin']
            final=render(spec,reports)['packets'][(6 if fee else 0)+1]
            edges=[dict(previous=packets[0],next=packets[1]),dict(previous=packets[1],next=packets[2]),
                   dict(previous=packets[2],next=final)]
            row=copy.deepcopy(records[-1]['stop_response']['workspace']);row.update(copy.deepcopy(final['body']))
            row['runtimeServices']=[r['stop_response']['workspace']['runtimeServices'][0] for r in records]
            original=copy.deepcopy(row)
            entry=m.Reentry(final,paths,transition=edges)
            with patch.object(m,'fresh_resources',side_effect=probe) as resources:
                registered(final,[row],reentry=entry);registered(final,[row],reentry=entry)
                entry.confirm(final,[row],WID)
                self.assertEqual(resources.call_count,6)
            self.assertEqual(row,original)
            for invalid in (edges[1:],list(reversed(edges)),edges+[edges[-1]]):
                with self.assertRaises(ValueError):m.Reentry(final,paths,transition=invalid)
            entry=m.Reentry(final,paths,transition=edges);row['runtimeServices'].pop(0)
            with self.assertRaises(ValueError):registered(final,[row],reentry=entry)


if __name__ == '__main__': unittest.main()
