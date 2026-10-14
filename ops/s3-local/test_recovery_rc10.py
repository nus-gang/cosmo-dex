import argparse
import copy
import errno
import json
import os
from pathlib import Path
import shlex
import tempfile
import unittest
from contextlib import contextmanager
from unittest.mock import Mock,patch
import chain_group as g
import port_release as ports
import planned_registration as planned
import failed_chain as failed
import workspace_reentry as reentry
from pid_mailbox import _write
from test_registration_render import fixture


class RecoveryTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.spec,self.reports=fixture()
        self.plan=planned.plan(self.spec)
        self.packets=self.plan['packets'][2:6]

    def test_before_keys_twelve_exact_commands_and_minimal_existing_updates(self):
        self.assertEqual(self.plan,planned.plan(self.spec))
        self.assertFalse(self.plan['key_creation_required']);self.assertFalse(self.plan['board_change_after_keys'])
        self.assertEqual(len(self.plan['packets']),12)
        listing=[]
        for i,p in enumerate(self.plan['packets']):
            kind,a,_=reentry.command_info(p)
            if kind=='chain':self.assertIsNone(a.peers);self.assertIsNotNone(a.topology_root)
            row=copy.deepcopy(p['body']);row.update(id=str(i),companyId='a479c266-538c-4ed6-bbf0-1f96e1f67b3e',projectId=planned.PROJECT,runtimeServices=[{'id':'unchanged-history'}])
            listing.append(row)
        self.assertEqual(planned.diff(self.plan,listing),[])
        listing[1]['runtimeConfig']['workspaceRuntime']['commands'][0]['command']='old'
        changes=planned.diff(self.plan,listing)
        self.assertEqual(len(changes),1);self.assertEqual(changes[0]['method'],'PATCH')
        self.assertEqual(changes[0]['service_rows_preserved'],[{'id':'unchanged-history'}])
        self.assertEqual(set(changes[0]['body']),{'runtimeConfig'})
        with self.assertRaises(ValueError):planned.plan(dict(self.spec,control_root=self.spec['profiles']['fee0']))

    def test_public_topology_is_new_input_not_a_key_dependent_command(self):
        from chain_cli import parse,resolve_peers
        from chain_topology import prepare
        spec=copy.deepcopy(self.spec);spec['profiles']={f'fee{f}':str(self.root/f'fee{f}') for f in (0,25)}
        packets=planned.plan(spec)['packets']
        for fee,offset in ((0,2),(25,8)):
            root=Path(spec['profiles'][f'fee{fee}']);root.mkdir(mode=0o700)
            report=copy.deepcopy(self.reports[f'fee{fee}']);report['root']=str(root)
            _write(root,'initialization.json',reentry.encode(report));value=planned.publish_topology(root)
            nodes=[]
            for i,p in enumerate(packets[offset:offset+4]):
                words=shlex.split(p['body']['runtimeConfig']['workspaceRuntime']['commands'][0]['command'])
                a=parse(words[4:]);self.assertEqual(resolve_peers(a),value['node_ids'])
                self.assertEqual(len(a.peers.split(',')),3)
                nodes.append(dict(node_id=value['node_ids'][i],argv=words[5:]))
            self.assertEqual(prepare(spec['python'],spec['candidate'],nodes,fee_bps=fee)['packets'],packets[offset:offset+4])
            with self.assertRaises(FileExistsError):planned.publish_topology(root)
            a.runtime_pin='b'*64
            with self.assertRaises(ValueError):resolve_peers(a)

    def nodes(self,coordinator,events,stop_failure=None):
        for i,p in enumerate(self.packets):
            _,a,endpoints=reentry.command_info(p)
            e=dict(control_plane_stop_verified=False,host_release_observations={},host_release_observations_complete=False)
            box=Mock();box.collect.return_value=[100+i,200+i]
            def process(pids,i=i):
                events.append(('process',i));return dict(schema='s3-local-process-probe/1',pids=list(pids),listed_pids_absent=True)
            def writer(home,i=i):
                events.append(('writer',i));return dict(schema='s3-local-chain-writer-probe/1',home=str(home),writer_lock_reacquired=True,continuous_exclusion_verified=False)
            coordinator.add(i,e,box,p,a,endpoints,process,writer)
            @contextmanager
            def node(i=i,e=e):
                try:yield
                finally:
                    events.append(('stop',i))
                    if i==stop_failure:e['stop_error']='STOP_UNCONFIRMED'
                    else:e['control_plane_stop_verified']=True
            yield node()

    def test_all_stops_before_observations_primary_error_and_archive_separate(self):
        events=[];primary=RuntimeError('private original');seen=None
        def probe(endpoints,stop):
            events.append(('ports',len(endpoints)))
            raise ports.PortFailure(dict(error='PORT_BUSY_DEADLINE',simultaneous_bind_verified=False))
        with patch.object(g.Coordinator,'observe',autospec=True,side_effect=lambda c:c.__class__.original(c,probe)),patch.object(g,'preserve') as archive:
            with self.assertRaises(RuntimeError) as caught:
                with g.group(self.root,self.packets) as (stack,c):
                    seen=c
                    for node in self.nodes(c,events):stack.enter_context(node)
                    raise primary
            self.assertIs(caught.exception,primary);archive.assert_not_called()
        self.assertEqual(events[:4],[('stop',i) for i in (3,2,1,0)])
        self.assertEqual([x for x in events if x[0]=='writer'],[('writer',i) for i in range(4)])
        value=json.loads((self.root/'chain-group-result.json').read_bytes())
        self.assertEqual(value['primary_error'],'BODY_FAILED');self.assertNotIn('private original',json.dumps(value))
        self.assertTrue((self.root/'chain-group-publication.json').exists())
        self.assertTrue(all(e['writer_release_verified'] for e in seen.report['nodes'].values()))

    def test_unconfirmed_stop_only_excludes_unsafe_node_and_all_stops_attempted(self):
        events=[]
        def probe(endpoints,stop):
            events.append(('ports',len(endpoints)))
            return dict(schema='s3-local-bounded-port-probe/1',endpoints=list(endpoints),simultaneous_bind_verified=True)
        with patch.object(g.Coordinator,'observe',autospec=True,side_effect=lambda c:c.__class__.original(c,probe)),patch.object(g,'preserve'):
            with self.assertRaisesRegex(ValueError,'CHAIN_GROUP_UNCONFIRMED'):
                with g.group(self.root,self.packets) as (stack,c):
                    for node in self.nodes(c,events,2):stack.enter_context(node)
        self.assertEqual(len([e for e in events if e[0]=='stop']),4)
        self.assertNotIn(('writer',2),events);self.assertNotIn(('process',2),events)
        self.assertIn(('ports',6),events)
        with self.assertRaises(ValueError):g.Coordinator(self.packets[:3])
        with self.assertRaises(ValueError):g.Coordinator([self.packets[0]]*4)

    def test_bounded_busy_probe_only_retries_eaddrinuse_and_closes_every_socket(self):
        for kind in ('release','busy','permission','interrupt','close'):
            at=[0];opened=[];n=[0]
            def sleep(seconds):at[0]+=int(seconds*1e9)
            def factory(*_):
                sock=Mock();opened.append(sock);n[0]+=1
                if kind=='busy' or kind=='release' and n[0]<=2:sock.bind.side_effect=OSError(errno.EADDRINUSE,'private')
                if kind=='permission':sock.bind.side_effect=PermissionError(errno.EACCES,'private')
                if kind=='interrupt':sock.bind.side_effect=KeyboardInterrupt()
                if kind=='close':sock.close.side_effect=OSError('private')
                return sock
            if kind=='release':
                result=ports._bounded_check([('127.0.0.1',26657)],factory,lambda:at[0],sleep,lambda:False)
                self.assertEqual(len(result['attempts']),3);self.assertTrue(result['simultaneous_bind_verified'])
            else:
                with self.assertRaises(ports.PortFailure) as error:ports._bounded_check([('127.0.0.1',26657)],factory,lambda:at[0],sleep,lambda:False)
                r=error.exception.report;self.assertFalse(r['simultaneous_bind_verified'])
                self.assertEqual(len(r['attempts']),241 if kind=='busy' else 1)
                self.assertNotIn('private',json.dumps(r))
            for sock in opened:sock.close.assert_called_once();sock.listen.assert_not_called();sock.connect.assert_not_called()
            self.assertLessEqual(at[0],60_000_000_000)


# Keep the production method while injecting the port syscall/clock layer only.
g.Coordinator.original=g.Coordinator.observe


class FailedChainTest(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name).resolve();self.root.chmod(0o700)
        self.value=failed.incident((failed.FIXTURE/'incident.json').read_bytes())
        mapping={}
        for n in self.value['nodes']:
            r=self.root/f'original-{n["index"]}';r.mkdir(mode=0o700)
            _write(r,'challenge',n['challenge'].encode());_write(r,'pids.json',n['pid_raw'].encode());mapping[n['mailbox_root']]=r
        original=failed._root
        self.patch=patch.object(failed,'_root',side_effect=lambda p:original(mapping.get(str(p),p)))
        self.patch.start();self.addCleanup(self.patch.stop);self.mapping=mapping
        self.path=failed.preserve(self.root/('failed-chain-rc9-'+failed.INCIDENT_SHA256[:16]))

    def test_exact_incident_archive_four_originals_untouched_no_normal_receipt(self):
        self.assertEqual(failed.load(self.path,self.path.parent),self.value)
        self.assertFalse(json.loads(self.path.read_bytes())['normal_receipt'])
        for n in self.value['nodes']:
            r=self.mapping[n['mailbox_root']]
            self.assertEqual(set(p.name for p in r.iterdir()),{'challenge','pids.json'})
            self.assertEqual((r/'pids.json').read_text(),n['pid_raw'])
        with self.assertRaises(ValueError):reentry.read_receipt(self.path)
        with self.assertRaises(ValueError):failed.preserve(self.path.parent)
        changed=copy.deepcopy(self.value);changed['nodes'][0]['index']=9
        with self.assertRaises(ValueError):failed.incident(reentry.encode(changed))

    def test_fresh_candidate_and_exact_service_history_required(self):
        spec,_=fixture();spec.update(candidate='/new/rc10',bundle='/new/bundle',artifacts='/new/artifacts',runtime_pin='f'*64,
            control_root='/private/rc10',native_decision_id='11111111-1111-4111-8111-111111111111',
            ceo_revision='22222222-2222-4222-8222-222222222222',cto_revision='33333333-3333-4333-8333-333333333333')
        packets=planned.plan(spec)['packets'][2:6]
        for n,packet in zip(self.value['nodes'],packets):
            transition=dict(previous=n['packet'],next=packet)
            entry=reentry.Reentry(packet,[],transition=transition,failed_chains=[self.path])
            workspace=copy.deepcopy(n['stop_response']['workspace']);workspace.update(copy.deepcopy(packet['body']))
            entry._snapshot(packet,workspace)
            self.assertEqual(entry.prior_services(),{entry.chain_failures[0].row['id']:entry.chain_failures[0].row})
            workspace['runtimeServices'][0]['status']='running'
            with self.assertRaises(ValueError):entry._snapshot(packet,workspace)
            with self.assertRaises(ValueError):reentry.Reentry(n['packet'],[],transition=dict(previous=n['packet'],next=n['packet']),failed_chains=[self.path])
        first=next(iter(self.mapping.values()));(first/'challenge').write_text('changed')
        with self.assertRaises(ValueError):failed.load(self.path,self.path.parent)

    def test_evidence_mutation_and_pending_never_repair_or_authorize(self):
        path=self.path.parent/'evidence'/'bootstrap-fetch-raw.txt'
        # Pick the actual empty snapshot filename from the pinned evidence set.
        name=next(k for k in self.value['files_sha256'] if 'snapshot-fetch.raw' in k)
        path=self.path.parent/'evidence'/name;before=path.read_bytes();path.write_bytes(before+b'changed')
        with self.assertRaises(ValueError):failed.load(self.path,self.path.parent)
        path.write_bytes(before)
        self.path.parent.with_name('.'+self.path.parent.name+'.pending').mkdir()
        with self.assertRaises(ValueError):failed.Failure(self.path,self.value['nodes'][0]['packet'])


if __name__=='__main__':unittest.main()
