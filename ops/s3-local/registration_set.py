"""Build the exact 12 manual Paperclip packets for fee0 and fee25.

This is a pure compiler from a bounded initialization-derived specification.
It performs no Paperclip request, registration, port bind, or service start.
"""
import copy
import hashlib
import json
from pathlib import Path

from chain_topology import prepare as prepare_topology
from offline_cli import parse as parse_worker
from web_cli import parse_web
from chain_cli import parse as parse_chain
from workspace_registration import prepare, prepare_web

ERROR = 'REGISTRATION_SET_REJECTED'


def _argv(value):
    if type(value) is not list or not value or any(type(x) is not str for x in value):
        raise ValueError(ERROR)
    return list(value)


def _same(left, right, names):
    if any(getattr(left, name) != getattr(right, name) for name in names):
        raise ValueError(ERROR)


def compile_set(spec):
    """Return canonical-ready packets; every command is fully materialized."""
    try:
        if type(spec) is not dict or set(spec) != {'schema', 'python', 'candidate', 'profiles'}:
            raise ValueError()
        if spec['schema'] != 's3-local-registration-input/1':
            raise ValueError()
        python, candidate = Path(spec['python']), Path(spec['candidate'])
        if any(not path.is_absolute() or '..' in path.parts for path in (python, candidate)):
            raise ValueError()
        if type(spec['profiles']) is not dict or set(spec['profiles']) != {'fee0', 'fee25'}:
            raise ValueError()
        packets, topologies = [], {}
        for fee in (0, 25):
            value = spec['profiles']['fee' + str(fee)]
            if type(value) is not dict or set(value) != {'worker_argv', 'web_argv', 'nodes'}:
                raise ValueError()
            worker_argv, web_argv = _argv(value['worker_argv']), _argv(value['web_argv'])
            if type(value['nodes']) is not list:
                raise ValueError()
            nodes = copy.deepcopy(value['nodes'])
            worker, _ = parse_worker(worker_argv, reviewed=True, managed=True)
            web, *_ = parse_web(['serve-web-reviewed', *web_argv])
            topology = prepare_topology(str(python), str(candidate), nodes, fee_bps=fee)
            chains = [parse_chain(['serve-chain-reviewed', *node['argv']]) for node in nodes]
            common = ('bundle', 'artifacts', 'input_set', 'effective_profile', 'runtime_pin',
                      'local_demo_profile')
            _same(worker, web, common)
            for chain in chains:
                _same(worker, chain, common)
            if worker.rpc != chains[0].rpc or web.bind != worker.bind:
                raise ValueError()
            profile_packets = [prepare(str(python), str(candidate), worker_argv, fee_bps=fee),
                               prepare_web(str(python), str(candidate), web_argv, fee_bps=fee),
                               *topology['packets']]
            if any(packet['requires_board_registration'] is not True or
                   packet['approval_verified'] is not False or
                   packet['starts_service'] is not False for packet in profile_packets):
                raise ValueError()
            packets.extend(profile_packets)
            topologies['fee' + str(fee)] = dict(topology, packets=[])  # avoid duplicate bytes
        names = [packet['body']['name'] for packet in packets]
        if len(packets) != 12 or len(set(names)) != 12:
            raise ValueError()
        packet_bytes = json.dumps(packets, ensure_ascii=True, sort_keys=True,
                                  separators=(',', ':')).encode()
        return {'schema': 's3-local-registration-set/1', 'packet_count': 12,
                'packet_sha256': hashlib.sha256(packet_bytes).hexdigest(),
                'profiles': topologies, 'packets': packets,
                'requires_board_registration': True,
                'approval_verified': False, 'starts_service': False}
    except (KeyError, TypeError, AttributeError, ValueError):
        raise ValueError(ERROR) from None
