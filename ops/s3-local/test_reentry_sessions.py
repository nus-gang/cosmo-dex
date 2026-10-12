import copy
import json
import os
import sys
import unittest
from unittest.mock import patch,Mock
from contextlib import contextmanager

import workspace_reentry as m
import test_workspace_reentry as core
from test_workspace_reentry import synthetic,probe
import test_web_session as web_fixture
import test_chain_session as chain_fixture
import managed_session
import test_managed_session
import restart_session_cli as cli
from test_registration_render import fixture
from registration_render import render,key_packets
from pid_mailbox import _write
from workspace_registration import prepare


class SessionsTest(core.ReentryTest):
    # Only the following cases are independent tests, not inherited core cases.
    def test_web_chain_real_mailbox_session_restart_and_denial(self):
        for fee in (0,25):
            for kind,fixture_type in (('web',web_fixture.WebSessionTest),('chain',chain_fixture.ChainSessionTest)):
                for drift in (False,True):
                    f=fixture_type();f.setup_case(fee);self.addCleanup(f.doCleanups)
                    listings=f.client.read_workspaces.side_effect
                    before=next(listings)[0];next(listings);after=next(listings)[0]
                    packet={'method':'POST','path':'/api/projects/'+m.PROJECT+'/workspaces',
                        'body':{k:before[k] for k in ('name','sourceType','cwd','isPrimary','runtimeConfig')},
                        'requires_board_registration':True,'approval_verified':False,'starts_service':False}
                    record=synthetic(packet);path=self.archive(record,f'{kind}-{fee}-{drift}')
                    entry=m.Reentry(packet,[path]);f.kw['reentry']=entry
                    prior=record['stop_response']['workspace'];final_read=copy.deepcopy(prior)
                    if drift:final_read['runtimeServices'][0]['status']='running'
                    f.client.read_workspaces.side_effect=[[prior],[prior],[final_read],[after]]
                    original_request=f.client.request.side_effect
                    def request(p):
                        response=original_request(p)
                        response['operation']['metadata']['runtimeServiceId']=after['runtimeServices'][0]['id']
                        return response
                    f.client.request.side_effect=request
                    with patch.object(m,'fresh_resources',side_effect=probe):
                        if drift:
                            with self.assertRaises(ValueError):f.run_case()
                            f.client.request.assert_not_called()
                        else:
                            result=f.run_case();self.assertTrue(result['host_release_observations_complete'])
                            self.assertEqual(f.client.request.call_count,2)
                            self.assertEqual(f.client.read_workspaces.call_count,4)
                            self.assertEqual(len(entry.observations),2)

    def test_worker_restart_uses_fresh_listing_after_release_checks(self):
        for fee in (0,25):
            f=test_managed_session.SessionTest();f.setup_case(fee)
            packet=prepare(sys.executable,'/candidate',f.argv,fee_bps=fee)
            record=synthetic(packet);path=self.archive(record,'worker'+str(fee));f.workspace=record['stop_response']['workspace']
            f.kw['reentry']=m.Reentry(packet,[path])
            with patch.object(m,'fresh_resources',side_effect=probe):
                with f.run_session():pass
            self.assertEqual(f.events,['broker-open','read','audit','read','audit','read','start','stop','broker-close'])

    def test_cli_all_four_modes_archive_before_same_path_next_nonce(self):
        spec,reports=fixture()
        packets=render(spec,reports)['packets']
        cases=[key_packets(spec)[0],packets[1],packets[0],packets[2]]
        cases=json.loads(json.dumps(cases).replace('/private/control',str(self.root)))
        for i,packet in enumerate(cases):
            kind,a,_=m.command_info(packet)
            a.pid_mailbox.parent.mkdir(mode=0o700,parents=True,exist_ok=True);a.pid_mailbox.parent.chmod(0o700)
            params=dict(schema='s3-local-reentry-session/1',packet=packet,workspace_id='11111111-1111-4111-8111-111111111111',
                duration_seconds=1,receipts=[],transition=None)
            nonces=[]
            @contextmanager
            def session(*args,**kw):
                box=(args[0]._mailbox if kind=='worker' else args[5] if kind=='keys' else kw['mailbox'])
                nonces.append(box.nonce);record=synthetic(packet,nonce=box.nonce)
                _write(box.root,'pids.json',record['pid_raw'].encode())
                evidence={};yield evidence
                pids=box.collect()
                evidence.update(control_plane_stop_verified=True,host_release_observations_complete=True,
                    pid_handoff_collected=True,listed_pids=list(pids),service_id=m.service_record(record)[0]['id'],
                    stop_response=record['stop_response'])
            name={'keys':'key_session','web':'web_session','worker':'worker_session','chain':'chain_session'}[kind]
            for j in range(2):
                with patch.object(cli,name,side_effect=session),patch.object(cli.time,'monotonic',side_effect=[1,3]):
                    result=cli.run(params,lambda:False)
                record,_=m.read_receipt(result['receipt']);self.assertEqual(record['packet'],packet)
                params['receipts']=[result['receipt']]
                self.assertFalse(a.pid_mailbox.exists())
            self.assertNotEqual(*nonces)

    def test_cli_denial_before_io_and_missing_approval_no_success(self):
        for args in ([],['run-reentry-reviewed'],['run-reentry-reviewed','--request','/missing'],['serve']):
            with patch.object(cli,'run') as run,patch('sys.stderr'):
                self.assertEqual(cli.main(args),2);run.assert_not_called()
        spec,reports=fixture();spec['control_root']=str(self.root);p=key_packets(spec)[0];a=m.command_info(p)[1]
        a.pid_mailbox.parent.mkdir(mode=0o700,parents=True);a.pid_mailbox.parent.chmod(0o700)
        request=dict(packet=p,workspace_id='11111111-1111-4111-8111-111111111111',duration_seconds=1,receipts=[],transition=None)
        with patch.object(cli.approval_gate,'inspect',side_effect=ValueError()),patch.object(cli.RuntimeClient,'from_environment') as client:
            with self.assertRaises(ValueError):cli.run(request,lambda:False)
            client.assert_not_called()
        self.assertEqual({x.name for x in a.pid_mailbox.iterdir()},{'challenge'})

# Avoid collecting inherited core cases twice.
for name in list(vars(core.ReentryTest)):
    if name.startswith('test_'):setattr(SessionsTest,name,None)

if __name__=='__main__':unittest.main()
