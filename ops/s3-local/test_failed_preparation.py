import argparse
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch,Mock

import failed_preparation as f
import workspace_reentry as m
from pid_mailbox import _write
from registration_render import key_packets
from workspace_registration import registered
from test_workspace_reentry import probe


class FailureTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.raw=(Path(f.__file__).parent/'fixtures/failed-preparation-rc8/incident.json').read_bytes()
        self.value,self.row,self.args=f.incident(self.raw)
        self.old=copy.deepcopy(self.value['packet'])
        old=self.args
        spec={k:str(getattr(old,k)) for k in ('bundle','artifacts','runtime_pin','native_decision_id','ceo_revision','cto_revision')}
        import shlex
        spec.update(python=shlex.split(self.old['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command'])[1],
            candidate='/fresh/rc9',control_root=str(self.root/'fresh'),bundle='/fresh/bundle',artifacts='/fresh/artifacts',runtime_pin='f'*64,
            native_decision_id='11111111-1111-4111-8111-111111111111',ceo_revision='22222222-2222-4222-8222-222222222222',cto_revision='33333333-3333-4333-8333-333333333333')
        self.packet=key_packets(spec)[0]
        self.spec=spec
        previous=[r['packet'] for r in self.value['prior_receipts']]
        self.edges=[dict(previous=previous[0],next=previous[1]),dict(previous=previous[1],next=previous[2]),dict(previous=self.old,next=self.packet)]
        self.paths=[]
        for i,record in enumerate(self.value['prior_receipts']):
            p=self.root/str(i);p.mkdir(mode=0o700);(p/'mailbox').mkdir(mode=0o700)
            _write(p/'mailbox','challenge',record['challenge'].encode());_write(p/'mailbox','pids.json',record['pid_raw'].encode())
            imported=copy.deepcopy(record);imported['archive_root']=str(p)
            _write(p,'receipt.json',m.encode(imported));self.paths.append(p/'receipt.json')
        # Only the OS path of the ORIGINAL failed mailbox is injected. The
        # production incident, v2 parsers, transition and all service comparisons run.
        self.original=self.root/'original';self.original.mkdir(mode=0o700)
        _write(self.original,'challenge',self.value['challenge'].encode())
        self.orig=f.original
        self.patcher=patch.object(f,'original',side_effect=lambda value,args:self.orig(value,argparse.Namespace(pid_mailbox=self.original)))
        self.patcher.start();self.addCleanup(self.patcher.stop)
        dest=self.root/('failed-'+self.value['challenge'])
        records={r['archive_root']:r for r in self.value['prior_receipts']}
        with patch.object(m,'read_receipt',side_effect=lambda p:(records[str(p.parent)],'unused')):
            self.failure=f.preserve(dest)
        self.workspace=copy.deepcopy(self.value['workspace']);self.workspace.update(copy.deepcopy(self.packet['body']))

    def entry(self,**kw):
        return m.Reentry(self.packet,self.paths,transition=self.edges,failed_preparations=[self.failure],**kw)

    def probes(self):
        return patch.object(f.Failure,'probe',return_value={'pid_source':'reviewed_control_plane_history','pid_handoff':'MISSING','normal_receipt':False})

    def test_actual_incident_preserves_three_v2_receipts_and_changed_service(self):
        before={p:str(p.read_bytes()) for p in self.original.iterdir()}
        with patch.object(m,'fresh_resources',side_effect=probe),self.probes():
            entry=self.entry();entry.begin()
            registered(self.packet,[self.workspace],reentry=entry)
            registered(self.packet,[self.workspace],reentry=entry)
            entry.final_check(self.packet,[self.workspace],self.workspace['id'],lambda:None)
            self.assertEqual(entry.prior_services(),{s['id']:s for s in self.workspace['runtimeServices']})
            self.assertEqual(len(entry.records),3);self.assertEqual(len(entry.observations),2)
            self.assertEqual(entry.observations[0][-1]['pid_handoff'],'MISSING')
        self.assertEqual(before,{p:str(p.read_bytes()) for p in self.original.iterdir()})
        self.assertFalse((self.original/'pids.json').exists())
        with self.assertRaises((ValueError,FileNotFoundError)):m.read_receipt(self.failure)
        with self.assertRaises(ValueError):registered(self.packet,[self.workspace])

    def test_all_service_rows_and_nonce_config_are_exact(self):
        edits=[lambda w:w['runtimeServices'].pop(),lambda w:w['runtimeServices'].append(copy.deepcopy(w['runtimeServices'][0])),
            lambda w:w['runtimeServices'][0].update(providerRef='999'),lambda w:w['runtimeServices'][0].update(status='running'),
            lambda w:w['runtimeServices'][1].update(command='other'),lambda w:w['runtimeConfig'].update(desiredState='running')]
        for change in edits:
            w=copy.deepcopy(self.workspace);change(w)
            with self.subTest(change=change),patch.object(m,'fresh_resources') as resources,self.assertRaises(ValueError):
                registered(self.packet,[w],reentry=self.entry())
            resources.assert_not_called()
        with self.assertRaises(ValueError):registered(self.packet,[self.workspace,self.workspace],reentry=self.entry())

    def test_same_candidate_empty_missing_receipt_or_transition_denied(self):
        for packet,paths,edges in ((self.old,self.paths,None),(self.packet,[],self.edges),
                (self.packet,self.paths[:2],self.edges),(self.packet,self.paths,None)):
            with self.assertRaises(ValueError):m.Reentry(packet,paths,transition=edges,failed_preparations=[self.failure])
        # The incident hash is a closed allowlist, not a generic success flag.
        for key,value in [('pid_handoff','COMPLETE'),('normal_receipt',True),('challenge','a'*64)]:
            data=copy.deepcopy(self.value);data[key]=value
            with self.assertRaises(ValueError):f.incident(m.encode(data))

    def test_partial_pending_link_and_original_repair_rejected(self):
        for name in ('pids.json','extra'):
            p=self.original/name;p.write_bytes(b'fake')
            with self.assertRaises(ValueError):self.entry()
            p.unlink() # Synthetic fixture only.
        challenge=self.original/'challenge';old=challenge.read_bytes();challenge.write_bytes(b'e'*64)
        with self.assertRaises(ValueError):self.entry()
        challenge.write_bytes(old)
        entry=self.entry();pending=self.failure.parent.with_name('.'+self.failure.parent.name+'.pending');pending.mkdir()
        with self.assertRaises(ValueError):entry._snapshot(self.packet,self.workspace)
        pending.rmdir()
        raw=self.failure.read_bytes();self.failure.write_bytes(b'{')
        with self.assertRaises(ValueError):self.entry()
        self.failure.write_bytes(raw)
        incident=self.failure.parent/'incident.json';saved=incident.with_name('saved');incident.rename(saved);incident.symlink_to('saved')
        with self.assertRaises(ValueError):self.entry()

    def test_failure_publication_collision_and_io_preserve_original(self):
        before=(self.original/'challenge').read_bytes()
        with self.assertRaises(ValueError):f.preserve(self.failure.parent)
        self.assertEqual((self.original/'challenge').read_bytes(),before)
        self.assertEqual(set(p.name for p in self.original.iterdir()),{'challenge'})

    def test_control_pid_is_observation_only_live_or_permission_denies(self):
        failure=f.Failure(self.failure)
        for cause in (ValueError(),PermissionError(),KeyboardInterrupt()):
            with patch('process_release.check',side_effect=cause) as process,patch('port_release.check') as port:
                with self.assertRaises(type(cause)):failure.probe()
                process.assert_called_once_with([15632,47145]);port.assert_not_called()

    def test_four_exact_edges_reach_final_web_without_dropping_failed_history(self):
        from test_registration_render import fixture
        from registration_render import render
        spec,reports=fixture();spec.update(self.spec)
        spec['control_root']='/private/rc9-test-control'  # Command bytes only; no directory or socket.
        for value in reports.values():value['runtime_pin']=spec['runtime_pin']
        preparation=key_packets(spec)[0]
        final=render(spec,reports)['packets'][1]
        transitions=self.edges[:-1]+[dict(previous=self.old,next=preparation),dict(previous=preparation,next=final)]
        entry=m.Reentry(final,self.paths,transition=transitions,failed_preparations=[self.failure])
        workspace=copy.deepcopy(self.workspace);workspace.update(copy.deepcopy(final['body']))
        with patch.object(m,'fresh_resources',side_effect=probe),self.probes():
            registered(final,[workspace],reentry=entry)
        self.assertEqual(len(entry.prior_services()),3)
        with self.assertRaises(ValueError):m.Reentry(final,self.paths,transition=transitions)


if __name__=='__main__':unittest.main()
