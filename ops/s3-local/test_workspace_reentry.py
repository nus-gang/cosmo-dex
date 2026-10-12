import copy
import hashlib
import json
import os
from pathlib import Path
import shlex
import tempfile
import time
import unittest
from unittest.mock import patch

import workspace_reentry as m
from workspace_registration import registered
from pid_mailbox import _write, Mailbox
from web_pid_mailbox import WebMailbox, _report
from runtime_evidence import stopped
from test_registration_render import fixture
from registration_render import render, key_packets
from native_review import COMPANY
from workspace_registration import PROJECT

F = Path(__file__).parent/'fixtures/stopped-workspace-rc4'
WID='11111111-1111-4111-8111-111111111111'
SID='33333333-3333-4333-8333-333333333333'
OID='22222222-2222-4222-8222-222222222222'


def probe(record):
    _,_,kind,a,ports,pids=m.service_record(record)
    result={'process':dict(schema='s3-local-process-probe/1',pids=list(pids),listed_pids_absent=True),
        'port':dict(schema='s3-local-port-probe/1',endpoints=list(ports),simultaneous_bind_verified=True)}
    if kind=='chain':result['writer']=dict(home=str(a.home),writer_lock_reacquired=True)
    if kind=='worker':result['writer']=dict(writer_reopen_verified=True)
    return result


def synthetic(packet, nonce='a'*64, sid=SID):
    kind,a,_=m.command_info(packet);c=packet['body']['runtimeConfig']['workspaceRuntime']['commands'][0]
    service=dict(id=sid,companyId=COMPANY,projectId=PROJECT,projectWorkspaceId=WID,
        scopeType='project_workspace',scopeId=WID,executionWorkspaceId=None,issueId=None,
        provider='local_process',lifecycle='shared',status='stopped',serviceName=c['name'],
        command=c['command'],cwd=c['cwd'],port=c['port'],url=None,providerRef='11111',
        startedAt='2026-10-12T16:00:00.000Z',stoppedAt='2026-10-12T16:01:00.000Z')
    workspace=dict(copy.deepcopy(packet['body']),id=WID,companyId=COMPANY,projectId=PROJECT,runtimeServices=[service])
    op=dict(id=OID,companyId=COMPANY,status='succeeded',phase='workspace_teardown',command=c['command'],cwd=c['cwd'],
        startedAt=service['stoppedAt'],finishedAt=service['stoppedAt'],metadata=dict(action='stop',projectId=PROJECT,
            projectWorkspaceId=WID,workspaceCommandId=c['id'],workspaceCommandKind='service',runtimeServiceId=sid))
    raw=(_report(nonce,11111) if kind in ('keys','web') else json.dumps(dict(schema='s3-local-pids/1',nonce=nonce,
        launcher_pid=11111,worker_pid=22222),sort_keys=True,separators=(',',':')).encode())
    return dict(schema='s3-local-stopped-receipt/1',packet=packet,stop_response=dict(workspace=workspace,operation=op),
        mailbox_root=str(a.pid_mailbox),challenge=nonce,pid_raw=raw.decode())


class ReentryTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)

    def archive(self,record,name='prior'):
        root=self.root/name;root.mkdir(mode=0o700);box=root/'mailbox';box.mkdir(mode=0o700)
        _write(box,'challenge',record['challenge'].encode());_write(box,'pids.json',record['pid_raw'].encode())
        _write(root,'receipt.json',m.encode(record));return root/'receipt.json'

    def test_original_stop_fixture_accepted_only_with_bound_evidence_and_fresh_probes(self):
        for name,digest in json.loads((F/'SHA256.json').read_text()).items():
            self.assertEqual(hashlib.sha256((F/name).read_bytes()).hexdigest(),digest)
        packet=json.loads((F/'registration-packet.json').read_text())
        before=json.loads((F/'attempt2__workspaces-pre-start.json').read_text())
        after=json.loads((F/'attempt2__workspaces-after-stop.json').read_text())
        stop=json.loads((F/'attempt2__stop-response.json').read_text())
        registered(packet,before)
        with self.assertRaises(ValueError):registered(packet,after)
        record=dict(schema='s3-local-stopped-receipt/1',packet=packet,stop_response=stop,
            mailbox_root=str(m.command_info(packet)[1].pid_mailbox),
            challenge=(F/'challenge').read_text(),pid_raw=(F/'pids.json').read_text())
        entry=m.Reentry(packet,[self.archive(record)])
        original=copy.deepcopy(after)
        with patch.object(m,'fresh_resources',side_effect=probe) as probes:
            a=registered(packet,after,reentry=entry);b=registered(packet,after,reentry=entry)
            self.assertEqual(a,b);self.assertEqual(probes.call_count,2)
            entry.confirm(packet,after,a['workspace_id'])
        self.assertEqual(after,original)
        with self.assertRaises(ValueError):registered(packet,after,reentry=entry)

    def test_foreign_unknown_changed_and_unbound_rows_fail_closed(self):
        spec,reports=fixture();packet=render(spec,reports)['packets'][1];record=synthetic(packet)
        path=self.archive(record);ws=record['stop_response']['workspace']
        for key,value in [('status','running'),('status','starting'),('status','unknown'),('scopeId',OID),
                ('providerRef','22222'),('command','other'),('port',9999),('stoppedAt',None),('id',OID)]:
            bad=copy.deepcopy(ws);bad['runtimeServices'][0][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):
                registered(packet,[bad],reentry=m.Reentry(packet,[path]))
        for rows in ([],ws['runtimeServices']*2):
            bad=copy.deepcopy(ws);bad['runtimeServices']=rows
            with self.assertRaises(ValueError):registered(packet,[bad],reentry=m.Reentry(packet,[path]))
        with self.assertRaises(ValueError):registered(packet,[ws],reentry={})

    def test_stop_identity_nonce_pid_and_scope_tampering_rejected(self):
        spec,reports=fixture();packet=render(spec,reports)['packets'][1]
        for index,modify in enumerate((
            lambda r:r['stop_response']['operation'].update(status='failed'),
            lambda r:r['stop_response']['operation']['metadata'].update(runtimeServiceId=OID),
            lambda r:r['stop_response']['workspace']['runtimeServices'][0].update(scopeType='issue'),
            lambda r:r.update(challenge='b'*64),
            lambda r:r['stop_response']['workspace']['runtimeServices'][0].update(providerRef='22222'))):
            r=synthetic(packet);modify(r)
            with self.assertRaises(ValueError):m.Reentry(packet,[self.archive(r,str(index))])

    def test_explicit_keys_to_final_and_all_history_rows_preserved(self):
        spec,reports=fixture();old=key_packets(spec)[0];new=render(spec,reports)['packets'][1]
        record=synthetic(old);path=self.archive(record)
        with self.assertRaises(ValueError):m.Reentry(new,[path])
        transition=dict(previous=old,next=new)
        entry=m.Reentry(new,[path],transition=transition)
        ws=copy.deepcopy(record['stop_response']['workspace']);ws['runtimeConfig']=new['body']['runtimeConfig']
        with patch.object(m,'fresh_resources',side_effect=probe):
            registered(new,[ws],reentry=entry);registered(new,[ws],reentry=entry);entry.confirm(new,[ws],WID)
        latest=synthetic(new,sid=OID);latest['stop_response']['workspace']['runtimeServices'].append(ws['runtimeServices'][0])
        final=latest['stop_response']['workspace']
        stopped(latest['stop_response'],[final],WID,new['body']['runtimeConfig'],history=entry)
        second=self.archive(latest,'latest')
        restarted=m.Reentry(new,[path,second],transition=transition)
        with patch.object(m,'fresh_resources',side_effect=probe):registered(new,[final],reentry=restarted)
        changed=copy.deepcopy(final);changed['runtimeServices'].pop()
        with self.assertRaises(ValueError):registered(new,[changed],reentry=m.Reentry(new,[path,second],transition=transition))
        mutated=copy.deepcopy(new);mutated['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command']+=' --arbitrary'
        with self.assertRaises(ValueError):m.Reentry(mutated,[path],transition=dict(previous=old,next=mutated))

    def test_writer_required_both_fees_no_stale_probe_or_retry(self):
        spec,reports=fixture();packets=render(spec,reports)['packets']
        for index in (0,2,6,8):
            packet=packets[index];record=synthetic(packet);path=self.archive(record,str(index));ws=record['stop_response']['workspace']
            with patch.object(m,'fresh_resources',side_effect=probe):registered(packet,[ws],reentry=m.Reentry(packet,[path]))
            bad=probe(record);bad.pop('writer')
            entry=m.Reentry(packet,[path])
            with patch.object(m,'fresh_resources',return_value=bad):
                with self.assertRaises((ValueError,KeyError)):registered(packet,[ws],reentry=entry)
                with self.assertRaises(ValueError):registered(packet,[ws],reentry=entry)
        packet=packets[1];record=synthetic(packet);path=self.archive(record,'web');entry=m.Reentry(packet,[path])
        with self.assertRaises(ValueError):entry._check(packet,record['stop_response']['workspace'],probe,iter([0,60_000_000_001]).__next__)

    def test_archive_original_bytes_fsync_no_replace_then_new_nonce(self):
        spec,reports=fixture();spec['control_root']=str(self.root)
        packet=key_packets(spec)[0];a=m.command_info(packet)[1];a.pid_mailbox.parent.mkdir(parents=True,mode=0o700)
        a.pid_mailbox.parent.chmod(0o700)
        box=WebMailbox(a.pid_mailbox);record=synthetic(packet,nonce=box.nonce)
        _write(box.root,'pids.json',record['pid_raw'].encode());pids=box.collect()
        evidence=dict(control_plane_stop_verified=True,host_release_observations_complete=True,pid_handoff_collected=True,
            listed_pids=list(pids),service_id=SID,stop_response=record['stop_response'])
        receipt=m.preserve(box,packet,evidence);self.assertEqual(m.read_receipt(receipt)[0],record)
        self.assertFalse(box.root.exists());fresh=WebMailbox(box.root);self.assertNotEqual(box.nonce,fresh.nonce)
        entry=m.Reentry(packet,[receipt]);entry.bind_mailbox(fresh)
        with self.assertRaises(ValueError):m.preserve(box,packet,evidence)
        (receipt.parent/'mailbox'/'pids.json').write_text('{}')
        with self.assertRaises(ValueError):entry.check(packet,record['stop_response']['workspace'])

    def test_final_listing_drift_and_io_interrupt_close_attempt(self):
        spec,reports=fixture();packet=render(spec,reports)['packets'][1];record=synthetic(packet)
        path=self.archive(record);ws=record['stop_response']['workspace']
        entry=m.Reentry(packet,[path])
        with patch.object(m,'fresh_resources',side_effect=probe):
            registered(packet,[ws],reentry=entry);registered(packet,[ws],reentry=entry)
        changed=copy.deepcopy(ws);changed['runtimeConfig']['desiredState']='running'
        with self.assertRaises(ValueError):entry.confirm(packet,[changed],WID)
        for error in (OSError(),KeyboardInterrupt()):
            entry=m.Reentry(packet,[path])
            with patch.object(m,'fresh_resources',side_effect=error):
                with self.assertRaises(type(error)):entry.check(packet,ws)
            with self.assertRaises(ValueError):entry.check(packet,ws)

    def test_revoked_preparation_upgrade_exact_pair_chain_and_original_copy(self):
        spec,reports=fixture();old=key_packets(spec)[0];first=synthetic(old)
        oldpath=self.archive(first,'old-candidate')
        spec.update(candidate='/new-candidate',bundle='/new-bundle',artifacts='/new-artifacts',
            runtime_pin='e'*64,native_decision_id='44444444-2222-4333-8444-555555555555',
            ceo_revision='55555555-2222-4333-8444-555555555555',cto_revision='66666666-2222-4333-8444-555555555555',
            control_root='/private/new-control')
        for report in reports.values():report['runtime_pin']=spec['runtime_pin']
        newkey=key_packets(spec)[0];final=render(spec,reports)['packets'][1]
        edges=[dict(previous=old,next=newkey),dict(previous=newkey,next=final)]
        second=synthetic(newkey,sid=OID);secondpath=self.archive(second,'new-candidate')
        row=copy.deepcopy(second['stop_response']['workspace']);row['runtimeServices'].append(first['stop_response']['workspace']['runtimeServices'][0])
        row['runtimeConfig']=final['body']['runtimeConfig']
        entry=m.Reentry(final,[oldpath,secondpath],transition=edges)
        with patch.object(m,'fresh_resources',side_effect=probe):registered(final,[row],reentry=entry)
        for bad in (list(reversed(edges)),[edges[1]],dict(previous=final,next=old)):
            with self.assertRaises(ValueError):m.Reentry(final,[oldpath,secondpath],transition=bad)
        # Real original bytes are copied, never moved or treated as current probes.
        packet=json.loads((F/'registration-packet.json').read_text())
        response=json.loads((F/'attempt2__stop-response.json').read_text())
        original_root=m.command_info(packet)[1].pid_mailbox
        # Use an isolated source with a packet generated for its mailbox.
        spec['control_root']=str(self.root);packet=key_packets(spec)[0];a=m.command_info(packet)[1]
        a.pid_mailbox.parent.mkdir(mode=0o700,parents=True);a.pid_mailbox.parent.chmod(0o700)
        box=WebMailbox(a.pid_mailbox);record=synthetic(packet,nonce=box.nonce)
        _write(box.root,'pids.json',record['pid_raw'].encode())
        saved={p.name:p.read_bytes() for p in box.root.iterdir()}
        copied=m.import_evidence(packet,record['stop_response'],box.root,self.root/'imported')
        self.assertEqual(m.read_receipt(copied)[0],record)
        self.assertEqual(saved,{p.name:p.read_bytes() for p in box.root.iterdir()})
        with self.assertRaises(FileExistsError):m.import_evidence(packet,record['stop_response'],box.root,self.root/'imported')

    def test_archive_fsync_failure_preserves_original_and_consumes_attempt(self):
        spec,_=fixture();spec['control_root']=str(self.root);packet=key_packets(spec)[0];a=m.command_info(packet)[1]
        a.pid_mailbox.parent.mkdir(mode=0o700,parents=True);a.pid_mailbox.parent.chmod(0o700)
        box=WebMailbox(a.pid_mailbox);record=synthetic(packet,nonce=box.nonce)
        _write(box.root,'pids.json',record['pid_raw'].encode());pids=box.collect()
        evidence=dict(control_plane_stop_verified=True,host_release_observations_complete=True,pid_handoff_collected=True,
            listed_pids=list(pids),service_id=SID,stop_response=record['stop_response'])
        with patch.object(m.os,'fsync',side_effect=OSError()):
            with self.assertRaises(ValueError):m.preserve(box,packet,evidence)
        self.assertEqual((box.root/'pids.json').read_text(),record['pid_raw'])
        with self.assertRaises(ValueError):m.preserve(box,packet,evidence)

if __name__=='__main__':unittest.main()
