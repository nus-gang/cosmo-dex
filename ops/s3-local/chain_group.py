"""Four-node L-T coordinator: STOP all once, then collect bounded observations.

The caller's primary error is re-raised unchanged. Evidence has separate body,
STOP, resource and publication errors. Failure snapshots never become receipts.
"""
from contextlib import ExitStack, contextmanager
import copy
from pathlib import Path
import port_release
from workspace_reentry import encode, preserve
from pid_mailbox import _root, _write


class Coordinator:
    def __init__(self, packets, stop=lambda:False):
        from workspace_reentry import command_info
        if not isinstance(packets,(tuple,list)) or len(packets)!=4:raise ValueError('CHAIN_GROUP_TARGET')
        self.packets=copy.deepcopy(packets)
        info=[command_info(p) for p in self.packets]
        if any(k!='chain' for k,_,_ in info) or len({p['body']['name'] for p in self.packets})!=4:
            raise ValueError('CHAIN_GROUP_TARGET')
        if len({x for _,a,_ in info for x in (a.rpc,a.p2p)})!=8:raise ValueError('CHAIN_GROUP_TARGET')
        self.nodes={};self.stop=stop;self.closed=False
        self.report=dict(schema='s3-local-chain-group/1', primary_error=None,
                         nodes={}, ports=None, cleanup_complete_verified=False)

    def add(self,index,evidence,mailbox,packet,args,endpoints,process,writer):
        if self.closed or index not in range(4) or index in self.nodes or packet!=self.packets[index]:raise ValueError('CHAIN_GROUP_TARGET')
        if any(packet['body']['name']==n['packet']['body']['name'] for n in self.nodes.values()):
            raise ValueError('CHAIN_GROUP_TARGET')
        self.nodes[index]=dict(evidence=evidence,mailbox=mailbox,packet=copy.deepcopy(packet),
            home=args.home,endpoints=endpoints,process=process,writer=writer)
        self.report['nodes'][str(index)]=evidence

    def observe(self, ports=port_release.bounded_check):
        if self.closed:raise ValueError('CHAIN_GROUP_USED')
        self.closed=True
        eligible=[]
        for index,node in self.nodes.items():
            e=node['evidence'];e['observation_errors']={}
            if e.get('control_plane_stop_verified') is not True:
                e['observation_errors']['prerequisite']='STOP_UNCONFIRMED';continue
            try:
                pids=node['mailbox'].collect();e['listed_pids']=list(pids);e['pid_handoff_collected']=True
                r=node['process'](pids)
                if r.get('schema')!='s3-local-process-probe/1' or r.get('pids')!=list(pids) or r.get('listed_pids_absent') is not True:
                    raise ValueError()
                e['host_release_observations']['process']=copy.deepcopy(r)
                eligible.append(index)
            except BaseException as error:
                e['observation_errors']['process']='INTERRUPTED' if isinstance(error,(KeyboardInterrupt,SystemExit)) else 'PROCESS_UNCONFIRMED'
        # A single global port budget; no per-node 60-second multiplication.
        endpoints=tuple(x for i in eligible for x in self.nodes[i]['endpoints'])
        if endpoints:
            try:
                r=ports(endpoints,self.stop)
                if r.get('schema')!='s3-local-bounded-port-probe/1' or r.get('endpoints')!=list(endpoints) or r.get('simultaneous_bind_verified') is not True:
                    raise ValueError()
                self.report['ports']=copy.deepcopy(r)
            except BaseException as error:
                self.report['ports']=copy.deepcopy(getattr(error,'report',{'error':'PORT_UNCONFIRMED','simultaneous_bind_verified':False}))
        for index in eligible:
            n=self.nodes[index];e=n['evidence']
            port_ok=self.report['ports'] and self.report['ports'].get('simultaneous_bind_verified') is True
            if port_ok:
                e['host_release_observations']['port']=dict(schema='s3-local-port-probe/1',endpoints=list(n['endpoints']),
                    simultaneous_bind_verified=True,source='chain_group_global_probe',reservation_retained=False)
            else:e['observation_errors']['port']='PORT_UNCONFIRMED'
            # The PID/STOP prerequisites hold. A busy TCP port does not suppress
            # the independent writer-lock observation.
            try:
                r=n['writer'](n['home'])
                if r.get('schema')!='s3-local-chain-writer-probe/1' or r.get('home')!=str(n['home']) or r.get('writer_lock_reacquired') is not True or r.get('continuous_exclusion_verified') is not False:
                    raise ValueError()
                e['host_release_observations']['writer']=copy.deepcopy(r);e['writer_release_verified']=True
            except BaseException as error:
                e['observation_errors']['writer']='INTERRUPTED' if isinstance(error,(KeyboardInterrupt,SystemExit)) else 'WRITER_UNCONFIRMED'
            e['host_release_observations_complete']=not e['observation_errors']
        return self.report

    def publish(self, root):
        # This snapshot is saved even when body/STOP/resource observations fail.
        root=_root(root)
        _write(root,'chain-group-result.json',encode(self.report))
        if self.report['primary_error'] is None and not any(k in self.report for k in ('stack_error','observation_error')) and len(self.nodes)==4:
            for index,n in self.nodes.items():
                e=n['evidence']
                if e.get('host_release_observations_complete') is True:
                    try:e['normal_receipt']=str(preserve(n['mailbox'],n['packet'],e))
                    except BaseException:e['publication_error']='ARCHIVE_UNCONFIRMED'
        _write(root,'chain-group-publication.json',encode(self.report))


@contextmanager
def group(evidence_root, packets, stop=lambda:False):
    coordinator=Coordinator(packets,stop);stack=ExitStack();primary=None
    try:
        # Clients enter authenticated sessions through this stack and MUST pass
        # coordinator=coordinator. Each session's finally only STOPs its target.
        yield stack,coordinator
    except BaseException as error:
        primary=error
        coordinator.report['primary_error']='INTERRUPTED' if isinstance(error,(KeyboardInterrupt,SystemExit)) else 'BODY_FAILED'
    finally:
        try:stack.close()
        except BaseException:
            coordinator.report['stack_error']='SESSION_EXIT_FAILED'
        try:coordinator.observe()
        except BaseException:coordinator.report['observation_error']='GROUP_OBSERVATION_FAILED'
        try:coordinator.publish(evidence_root)
        except BaseException:
            coordinator.report['publication_error']='GROUP_PUBLICATION_FAILED'
        if primary is not None:raise primary
        if (len(coordinator.nodes)!=4 or any(e.get('host_release_observations_complete') is not True or
                e.get('publication_error') for e in coordinator.report['nodes'].values()) or
                any(k in coordinator.report for k in ('stack_error','observation_error','publication_error'))):
            raise ValueError('CHAIN_GROUP_UNCONFIRMED')
