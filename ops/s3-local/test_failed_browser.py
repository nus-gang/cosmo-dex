import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import failed_browser as f
import browser_inventory
import workspace_reentry as r
import planned_registration
from test_registration_render import fixture
from pid_mailbox import _write


class FailedBrowserTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.value=json.loads((f.FIXTURE/'incident.json').read_text());self.record=self.value['record']
        mail=self.root/'original-mail';mail.mkdir(mode=0o700)
        for name,raw in [('challenge',self.record['challenge']),('pids.json',self.record['pid_raw'])]:_write(mail,name,raw.encode())
        browser=self.root/'original-browser';browser.mkdir(mode=0o700)
        for name in self.value['original_event_files']:_write(browser,name,(f.FIXTURE/'evidence/browser-events'/name).read_bytes())
        for name in ('commands','results'):
            (browser/name).mkdir(mode=0o700)
            for file in self.value['original_'+name+'_files']:_write(browser/name,file,(f.FIXTURE/'evidence'/('browser-'+name)/file).read_bytes())
        self.mapping={self.record['mailbox_root']:mail,self.value['original_browser_root']:browser}
        original=f._root
        def mapped(p):return original(self.mapping.get(str(p),p))
        for module in (f,browser_inventory):
            hook=patch.object(module,'_root',side_effect=mapped);hook.start();self.addCleanup(hook.stop)
        self.path=f.preserve(self.root/('failed-browser-rc10-'+f.INCIDENT_SHA256[:16]))

    def test_exact_prekey_failure_archive_never_normal_receipt_or_repair(self):
        self.assertEqual(f.load(self.path,self.path.parent),self.value)
        self.assertFalse(json.loads(self.path.read_text())['normal_receipt'])
        with self.assertRaises(ValueError):r.read_receipt(self.path)
        with self.assertRaises(ValueError):f.preserve(self.path.parent)
        self.assertEqual(set(p.name for p in self.mapping[self.record['mailbox_root']].iterdir()),{'challenge','pids.json'})
        changed=(f.FIXTURE/'incident.json').read_bytes()+b' '
        with self.assertRaises(ValueError):f.incident(changed)
        finish=self.mapping[self.value['original_browser_root']]/'commands'/self.value['original_commands_files'][0]
        raw=finish.read_bytes();finish.unlink()
        with self.assertRaises(ValueError):f.load(self.path,self.path.parent)
        _write(finish.parent,finish.name,raw)

    def test_original_and_copied_evidence_mutation_missing_extra_and_pending_rejected(self):
        for path in [self.mapping[self.record['mailbox_root']]/'challenge',self.mapping[self.value['original_browser_root']]/'00001.json',self.path.parent/'evidence/postflight.json']:
            before=path.read_bytes();path.write_bytes(before+b'changed')
            with self.assertRaises(ValueError):f.load(self.path,self.path.parent)
            path.write_bytes(before)
        extra=self.mapping[self.value['original_browser_root']]/'registration.json';extra.write_text('{}')
        with self.assertRaises(ValueError):f.load(self.path,self.path.parent)
        extra.unlink()
        self.path.parent.with_name('.'+self.path.parent.name+'.pending').mkdir()
        with self.assertRaises(ValueError):f.Failure(self.path)

    def entry(self,mutate=None):
        h=json.loads((f.FIXTURE/'test-history.json').read_text());spec,_=fixture()
        spec.update(candidate='/new/rc11',bundle='/new/bundle',artifacts='/new/artifacts',runtime_pin='f'*64,
            control_root='/private/rc11',native_decision_id='11111111-1111-4111-8111-111111111111',
            ceo_revision='22222222-2222-4222-8222-222222222222',cto_revision='33333333-3333-4333-8333-333333333333')
        next_packet=planned_registration.plan(spec)['packets'][1]
        paths=[self.root/f'{i}/receipt.json' for i in range(4)];records=dict(zip(paths,[(x,'digest') for x in h['normal_records']]))
        # The older RC8 failure is already separately regression-tested. This
        # adapter retains its exact replacement row; RC10 cannot erase it.
        class PriorFailure:
            def __init__(this,*_):
                this.value=dict(workspace_id=self.record['workspace_id'],challenge='0'*64)
                this.row=next(x for x in self.value['prior_services'] if x['id']=='89cf8ef6-e801-48b4-bc81-95da712047f5')
            def check(this,*_):pass
            def probe(this):return {}
        transitions=h['transitions']+[dict(previous=self.record['packet'],next=next_packet)]
        if mutate:mutate(next_packet,transitions)
        with patch.object(r,'read_receipt',side_effect=lambda p:records[Path(p)]),patch('failed_preparation.Failure',PriorFailure):
            entry=r.Reentry(next_packet,paths,transition=transitions,failed_preparations=[self.root/'old-failure'],failed_browsers=[self.path])
        entry._test_records=records
        return entry

    def test_reentry_requires_all_five_exact_rows_and_new_candidate(self):
        entry=self.entry();workspace=copy.deepcopy(self.record['stop_response']['workspace']);workspace.update(entry.packet['body'])
        with patch.object(r,'read_receipt',side_effect=lambda p:entry._test_records[Path(p)]):
            entry._snapshot(entry.packet,workspace)
            self.assertEqual(len(entry.prior_services()),5)
            for mode in ('missing','duplicate','extra','running','changed'):
                changed=copy.deepcopy(workspace)
                if mode=='missing':changed['runtimeServices'].pop(0)
                elif mode=='duplicate':changed['runtimeServices'].append(changed['runtimeServices'][0])
                elif mode=='extra':changed['runtimeServices'].append(dict(id='unreviewed'))
                elif mode=='running':changed['runtimeServices'][-1]['status']='running'
                else:changed['runtimeServices'][0]['providerRef']='99999'
                with self.assertRaises(ValueError):entry._snapshot(entry.packet,changed)
        with self.assertRaises(ValueError):self.entry(lambda p,t:t[-1].update(next=self.record['packet']))

    def test_current_process_and_port_observations_required_with_no_tree_claim(self):
        failure=f.Failure(self.path)
        report=dict(process=dict(schema='s3-local-process-probe/1',pids=[22228,22324,22325],listed_pids_absent=True),
            port=dict(schema='s3-local-port-probe/1',endpoints=[('127.0.0.1',5173)],simultaneous_bind_verified=True))
        with patch.object(r,'fresh_resources',return_value=report) as probe:
            result=failure.probe();self.assertFalse(result['normal_receipt']);self.assertFalse(result['cleanup_complete_verified']);probe.assert_called_once()
        for key,field in [('process','listed_pids_absent'),('port','simultaneous_bind_verified')]:
            bad=copy.deepcopy(report);bad[key][field]=False
            with patch.object(r,'fresh_resources',return_value=bad),self.assertRaises(ValueError):failure.probe()
        with patch.object(r,'fresh_resources',side_effect=PermissionError()),self.assertRaises(PermissionError):failure.probe()


if __name__=='__main__':unittest.main()
