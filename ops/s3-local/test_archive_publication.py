"""Real filesystem/process exits, synthetic lifecycle; no product service IO."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import workspace_reentry as m
from pid_mailbox import Mailbox, _write, _archive_pending
from web_pid_mailbox import WebMailbox
from test_workspace_reentry import synthetic, probe
from test_registration_render import fixture
from registration_render import render, key_packets


def setup(root, kind, fee):
    spec,reports=fixture()
    packet=(key_packets(spec)[int(bool(fee))] if kind=='keys' else
        render(spec,reports)['packets'][(6 if fee else 0)+{'worker':0,'web':1,'chain':2}[kind]])
    if kind=='keys':
        spec['control_root']=str(root);packet=key_packets(spec)[int(bool(fee))]
    else:
        previous=m.command_info(packet)[1].pid_mailbox
        packet=json.loads(json.dumps(packet).replace(str(previous),str(root/'mailbox')))
    a=m.command_info(packet)[1]
    a.pid_mailbox.parent.mkdir(mode=0o700,parents=True,exist_ok=True);a.pid_mailbox.parent.chmod(0o700)
    box=(Mailbox if kind in ('worker','chain') else WebMailbox)(a.pid_mailbox)
    record=synthetic(packet,nonce=box.nonce);_write(box.root,'pids.json',record['pid_raw'].encode());pids=box.collect()
    evidence=dict(control_plane_stop_verified=True,host_release_observations_complete=True,
        pid_handoff_collected=True,listed_pids=list(pids),service_id=m.service_record(record)[0]['id'],
        stop_response=record['stop_response'])
    return box,packet,record,evidence


def child_fault(box,packet,evidence,point,mode):
    actual_sync=os.fsync;actual_rename=m._rename_exclusive;counts={'sync':0,'rename':0}
    def hit(label):
        if label!=point:return
        if mode=='exit':os._exit(71)
        if mode=='interrupt':raise KeyboardInterrupt()
        raise OSError('injected boundary')
    def call(name,actual,*args):
        counts[name]+=1;label=f'{name}{counts[name]}'
        hit(label+'-before');value=actual(*args);hit(label+'-after');return value
    with patch.object(m.os,'fsync',side_effect=lambda fd:call('sync',actual_sync,fd)), \
            patch.object(m,'_rename_exclusive',side_effect=lambda *args:call('rename',actual_rename,*args)):
        try:m.preserve(box,packet,evidence)
        except (ValueError,KeyboardInterrupt):os._exit(71)
    os._exit(0)


def observe(path):
    spec=json.loads(Path(path).read_text());packet=spec['packet'];record=spec['record']
    root=Path(record['mailbox_root']);archive=root.parent/('completed-'+record['challenge'])
    cls=Mailbox if m.command_info(packet)[0] in ('worker','chain') else WebMailbox
    if spec['committed']:
        value,_=m.read_receipt(archive/'receipt.json');assert value['pid_raw']==record['pid_raw']
        fresh=cls(root);assert fresh.nonce!=record['challenge']
        entry=m.Reentry(packet,[archive/'receipt.json']);entry.bind_mailbox(fresh)
        from workspace_registration import registered
        with patch.object(m,'fresh_resources',side_effect=probe):
            listing=[record['stop_response']['workspace']]
            registered(packet,listing,reentry=entry);registered(packet,listing,reentry=entry)
            entry.confirm(packet,listing,m.service_record(record)[1])
    else:
        for receipt in list(root.parent.rglob('receipt.json'))+[archive/'receipt.json']:
            try:m.read_receipt(receipt)
            except (ValueError,OSError,KeyError):pass
            else:raise AssertionError('incomplete receipt read')
            try:m.Reentry(packet,[receipt])
            except ValueError:pass
            else:raise AssertionError('incomplete reentry')
        try:cls(root)
        except ValueError:pass
        else:raise AssertionError('new nonce after failed preservation')
        original=root if root.exists() else _archive_pending(root)/'mailbox'
        assert (original/'pids.json').read_text()==record['pid_raw']
        assert (original/'challenge').read_text()==record['challenge']
        import restart_session_cli as cli
        req=dict(packet=packet,workspace_id=m.service_record(record)[1],duration_seconds=1,receipts=[],transition=None)
        with patch.object(cli,'key_session') as a,patch.object(cli,'web_session') as b, \
             patch.object(cli,'chain_session') as c,patch.object(cli,'worker_session') as d:
            try:cli.run(req,lambda:False)
            except ValueError:pass
            else:raise AssertionError('empty-receipt bypass')
            for mock in (a,b,c,d):mock.assert_not_called()


class PublicationTest(unittest.TestCase):
    def case(self,kind,fee,point,mode,committed):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();root.chmod(0o700)
            box,packet,record,evidence=setup(root,kind,fee)
            pid=os.fork()
            if pid==0:child_fault(box,packet,evidence,point,mode)
            _,status=os.waitpid(pid,0)
            self.assertEqual(os.waitstatus_to_exitcode(status),0 if point=='success' else 71)
            request=root/'observe.json';request.write_text(json.dumps(dict(packet=packet,record=record,committed=committed)))
            result=subprocess.run([sys.executable,'-B',__file__,'observe',str(request)],capture_output=True,timeout=10)
            self.assertEqual(result.returncode,0,result.stderr.decode())

    def test_all_sync_and_move_boundaries_survive_process_reentry(self):
        cases=0
        points=[f'sync{i}-{side}' for i in range(1,6) for side in ('before','after')]
        points+=['rename1-before','rename1-after','rename2-before']
        for fee in (0,25):
            for kind in ('keys','web','worker','chain'):
                for point in points:
                    for mode in ('error','interrupt','exit'):
                        with self.subTest(fee=fee,kind=kind,point=point,mode=mode):
                            self.case(kind,fee,point,mode,False);cases+=1
        print('failed publication boundaries, new-process denial:',cases,flush=True)

    def test_normal_and_exit_after_final_atomic_publication(self):
        cases=0
        for fee in (0,25):
            for kind in ('keys','web','worker','chain'):
                for point in ('success','rename2-after'):
                    with self.subTest(fee=fee,kind=kind,point=point):
                        self.case(kind,fee,point,'exit',True);cases+=1
        print('committed publication, new-process acceptance:',cases,flush=True)

    def test_legacy_and_existing_receipt_rejected_while_pending(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();root.chmod(0o700)
            box,packet,record,evidence=setup(root,'keys',0)
            receipt=m.preserve(box,packet,evidence)
            original=receipt.read_bytes();legacy=json.loads(original)
            legacy['schema']='s3-local-stopped-receipt/1'
            receipt.write_bytes(m.encode(legacy))
            with self.assertRaises(ValueError):m.read_receipt(receipt)
            receipt.write_bytes(original)
            _archive_pending(box.root).mkdir(mode=0o700)
            with self.assertRaises(ValueError):m.read_receipt(receipt)
            with self.assertRaises(ValueError):WebMailbox(box.root)

    def test_no_replace_and_import_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp).resolve();root.chmod(0o700)
            box,packet,record,evidence=setup(root,'keys',0)
            final=box.root.parent/('completed-'+box.nonce);final.mkdir(mode=0o700);(final/'keep').write_text('preserved')
            with self.assertRaises(ValueError):m.preserve(box,packet,evidence)
            self.assertEqual((final/'keep').read_text(),'preserved')
            with self.assertRaises(ValueError):WebMailbox(box.root)
        for index in range(1,9):
            with tempfile.TemporaryDirectory() as temp:
                root=Path(temp).resolve();root.chmod(0o700)
                box,packet,record,evidence=setup(root,'web',25)
                saved={p.name:p.read_bytes() for p in box.root.iterdir()};actual=os.fsync;count=0
                def sync(fd):
                    nonlocal count
                    count+=1
                    if count==index:raise OSError('import sync')
                    return actual(fd)
                with patch.object(m.os,'fsync',side_effect=sync):
                    with self.assertRaises(OSError):m.import_evidence(packet,record['stop_response'],box.root,root/'imported')
                self.assertEqual(saved,{p.name:p.read_bytes() for p in box.root.iterdir()})
                for p in root.rglob('receipt.json'):
                    with self.assertRaises((ValueError,KeyError)):m.read_receipt(p)


if __name__=='__main__':
    if sys.argv[1:2]==['observe']:observe(sys.argv[2])
    else:unittest.main()
