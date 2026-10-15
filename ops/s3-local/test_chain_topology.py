import copy
import sys
import unittest
import chain_topology as t
import test_chain_cli as fixtures


def nodes():
    result=[]
    for i in range(4):
        argv=fixtures.ChainCliTest.args(None)[1:]
        values=dict(home=f'/private/homes/v{i}',scratch=f'/private/scratch/v{i}',
                    pid_mailbox=f'/private/mail/v{i}',approval_socket=f'/private/broker/v{i}/s',
                    rpc=f'127.0.0.1:{28000+i*2}',p2p=f'127.0.0.1:{28001+i*2}',
                    peers=','.join(str(j+1)*40+f'@127.0.0.1:{28001+j*2}' for j in range(4) if j!=i))
        for k,v in values.items():argv[argv.index('--'+k.replace('_','-'))+1]=v
        result.append(dict(node_id=str(i+1)*40,argv=argv))
    return result


def change(n,i,key,value):n[i]['argv'][n[i]['argv'].index('--'+key)+1]=value


class TopologyTest(unittest.TestCase):
    def prepare(self,n,fee=0):return t.prepare(sys.executable,'/candidate',n,fee_bps=fee)
    def test_fee_profiles_packets_and_copy(self):
        for fee in (0,25):
            n=nodes(); r=self.prepare(n,fee)
            self.assertEqual(len(r['packets']),4);self.assertEqual(len(set(r['endpoints'])),8)
            self.assertFalse(r['starts_service']);self.assertFalse(r['home_identity_verified'])
            self.assertFalse(r['ports_available_verified']);self.assertFalse(r['approval_verified'])
            saved=copy.deepcopy(r);change(n,0,'home','/changed');self.assertEqual(r,saved)
            self.assertEqual([p['body']['name'] for p in r['packets']],
                             [f'nus-s3-local-chain-fee{fee}-v{i}' for i in range(4)])
    def test_peer_identity_and_cross_node_ports(self):
        cases=[]
        n=nodes();n[1]['node_id']=n[0]['node_id'];cases.append(n)
        n=nodes();n[0]['node_id']='a'*40;cases.append(n)
        n=nodes();change(n,1,'rpc','127.0.0.1:28000');cases.append(n)
        n=nodes();change(n,0,'peers',','.join(str(j+1)*40+f'@127.0.0.1:{29001+j*2}' for j in (1,2,3)));cases.append(n)
        for n in cases:
            with self.assertRaisesRegex(ValueError,t.ERROR):self.prepare(n)
    def test_mutable_roots_and_shared_candidate(self):
        for key,value in [('home','/private/homes/v0'),('scratch','/private/homes/v0/child'),
                          ('pid-mailbox','/private/homes'),('approval-socket','/private/mail/v0/s'),
                          ('home','/private/bundle'),('runtime-pin','b'*64),
                          ('input-set','/other'),('effective-profile','/other'),
                          ('lifetime-seconds','299')]:
            n=nodes();change(n,1,key,value)
            with self.assertRaisesRegex(ValueError,t.ERROR):self.prepare(n)
    def test_shape_and_opt_in_rejection(self):
        for n in ([],nodes()[:3],nodes()+nodes()[:1],None):
            with self.assertRaisesRegex(ValueError,t.ERROR):self.prepare(n)
        n=nodes();n[0]['argv'].remove('--acknowledge-unproven-space')
        with self.assertRaisesRegex(ValueError,t.ERROR):self.prepare(n)
        with self.assertRaisesRegex(ValueError,t.ERROR):self.prepare(nodes(),True)

if __name__=='__main__':unittest.main()
