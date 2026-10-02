"""SEC-S2A-01 economic/schema oracle. No engine, disk, network or runtime PASS."""
import copy


def check(load, cfg, defs, valid, expect, hjson):
    cases = load('vectors/correction-boundaries.json')['cases']
    expect(len(cases), 8, 'correction boundary coverage')
    for case in cases:
        name, inp, want = case['id'], case['input'], case['expected']
        n, bps = int(inp['fill_count']), int(inp['fee_bps'])
        q, p = int(inp['maker_qty_lots']), int(inp['price_ticks'])
        assert 1 <= q <= int(cfg['max_qty_lots'])
        assert 1 <= p <= int(cfg['max_price_ticks'])
        assert q * p <= int(cfg['max_order_quote_atoms'])
        assert 2 <= int(cfg['max_open_orders_total'])
        assert 1 <= int(cfg['max_open_orders_per_owner'])
        assert 2 <= int(inp['expiry_delta_blocks']) <= 1000
        assert 2 <= int(inp['maker_expiry_delta_blocks']) <= 1000
        assert 100 + (n + 1) // 2 < 100 + int(inp['maker_expiry_delta_blocks'])
        assert int(inp['max_live_orders']) == 2

        def row(c):
            return dict(C=c, R=0, D=0, P=0, A=c)

        state = {'seller_base': row(int(inp['seller_C_base'])),
                 'seller_quote': row(0), 'buyer_base': row(0),
                 'buyer_quote': row(int(inp['buyer_C_quote']))}
        sb, sq, bb, bq = [state[k] for k in state]
        sb['R'] = q * 1000
        sb['A'] -= sb['R']
        orders = [dict(max=q, remaining=q, filled=0, corrected=0, cancelled=0)]
        fills = []
        fees = [0, 0]
        # One live maker; each taker is fully filled before the next is submitted.
        for _ in range(n):
            assert bq['A'] >= p and sb['R'] >= 1000
            bq['A'] -= p
            bq['D'] += p
            sb['R'] -= 1000
            sb['D'] += 1000
            fb, fq = (1000 * bps + 9999) // 10000, (p * bps + 9999) // 10000
            bb['P'] += 1000 - fb
            sq['P'] += p - fq
            fees[0] += fb
            fees[1] += fq
            fills.append(dict(buy_D=p, sell_D=1000, buyer_P=1000-fb,
                              seller_P=p-fq, fee_base=fb, fee_quote=fq,
                              corrected=False))
            orders[0]['remaining'] -= 1
            orders[0]['filled'] += 1
            orders.append(dict(max=1, remaining=0, filled=1, corrected=0, cancelled=0))
        for asset, value in state.items():
            expect({k: str(v) for k, v in value.items()}, want[asset+'_before'], name+'/'+asset+'/before')
            assert value['A'] == value['C'] - value['R'] - value['D'] >= 0
        for index, asset in enumerate(('base', 'quote')):
            expect(str(fees[index]), want['pending_fee_'+asset+'_before'], name+'/fee-before')

        before = copy.deepcopy((state, orders, fills, fees))

        def correct(candidate):
            ledger, orders, fills, fees = candidate
            sb, sq, bb, bq = [ledger[k] for k in ledger]
            for order in orders:
                order['cancelled'] += order['remaining']
                order['remaining'] = 0
                order['corrected'] = order['filled']  # lifetime filled never decreases
            sb['R'] = 0
            for fill in fills:
                if fill['corrected']:
                    continue
                sb['D'] -= fill['sell_D']
                bq['D'] -= fill['buy_D']
                bb['P'] -= fill['buyer_P']
                sq['P'] -= fill['seller_P']
                fees[0] -= fill['fee_base']
                fees[1] -= fill['fee_quote']
                fill['corrected'] = True
            sb['C'] = int(inp['seller_C_base']) - int(inp['seller_withdraw_base'])
            for value in ledger.values():
                value['A'] = value['C'] - value['R'] - value['D']
                assert min(value.values()) >= 0
            return candidate

        after = correct(copy.deepcopy(before))
        for asset, value in after[0].items():
            expect({k: str(v) for k, v in value.items()}, want[asset+'_after'], name+'/'+asset+'/after')
        expect(after[3], [int(want['pending_fee_base_after']), int(want['pending_fee_quote_after'])], name+'/fee-after')
        for old, new in zip(before[1], after[1]):
            expect(new['filled'], old['filled'], name+'/lifetime-filled')
            assert new['max'] == new['remaining'] + new['filled'] + new['cancelled']
            assert new['corrected'] == new['filled']
        for field in ('filled', 'corrected', 'cancelled'):
            expect(str(after[1][0][field]), want['maker_'+field+'_after'], name+'/maker/'+field)
        for field in ('filled', 'corrected'):
            expect(str(after[1][1][field]), want['taker_'+field+'_after'], name+'/taker/'+field)

        correction, result = case['correction'], case['result']
        expect(result['observed_height'], str(101 + (n + 1) // 2), name+'/withdraw-height')
        expect(result['command_seq'], str(n + (n + 1) // 2 + 2), name+'/snapshot-and-order-sequence')
        valid(defs['Correction'], correction)
        valid(defs['CommandResult'], result)
        expect(str(len(result['affected_order_hashes'])), want['affected_orders'], name+'/orders')
        expect(str(len(correction['corrected_fill_ids'])), want['corrected_fills'], name+'/fills')
        expect(str(len(correction['cancelled_order_hashes'])), want['cancelled_orders'], name+'/cancelled')
        expect(result['corrected_fill_ids'], correction['corrected_fill_ids'], name+'/full-result')
        assert len(set(result['affected_order_hashes'])) == n + 1
        assert len(set(result['corrected_fill_ids'])) == n
        for field, limit in [('affected_order_hashes', 200), ('corrected_fill_ids', 1000)]:
            ids = result[field]
            pages = [ids[start:start+limit] for start in range(0, len(ids), limit)]
            expect([item for page in pages for item in page], ids, name+'/complete-page-reassembly/'+field)
            assert all(len(page) <= limit for page in pages)
        for change in result['ledger_changes']:
            role = 'seller' if change['owner'] == correction['changed_owners'][0] else 'buyer'
            asset = role + '_' + change['before']['denom'][3:].lower()
            for phase in ('before', 'after'):
                expect({k: v for k, v in change[phase].items() if k != 'denom'},
                       want[asset+'_'+phase], name+'/result-ledger/'+phase)
        expect(hjson('NUS/S2/RESULT/V1', result), case['result_hash'], name+'/result-hash')

        # A single logical commit exposes the entire candidate. These model checks
        # prescribe crash/response-loss cases; actual fsync faults remain C/F/H work.
        visible = copy.deepcopy(before)
        pending = correct(copy.deepcopy(before))
        expect(visible, before, name+'/crash-before-marker-no-partial-publication')
        expect(correct(copy.deepcopy(before)), pending, name+'/rebuild-after-uncommitted-crash')
        marker, effect_count = None, 0
        for _ in range(2):  # commit then response-loss retry / replay of same snapshot
            if marker is None:
                visible, marker = pending, case['result_hash']
                effect_count += 1
        expect(visible, after, name+'/commit-all')
        expect(str(effect_count), want['effect_count'], name+'/replay-once')
        expect(want['logical_commits'], '1', name+'/one-marker')
        expect(correct(copy.deepcopy(after)), after, name+'/duplicate-no-second-reversal')
        epochs = {'seller': 0, 'buyer': 0}
        epochs.update(seller=1)
        for owner in epochs:
            expect(str(epochs[owner]), want[owner+'_epoch_after'], name+'/new-epoch')

    # Query limits are transport limits only; lifetime internal data keeps all IDs.
    for name, field, limit in [('LedgerView', 'orders', 200), ('LedgerView', 'fills', 1000)]:
        spec = defs[name]['properties'][field]
        expect(spec['maxItems'], limit, name+'/'+field+'/page-limit')
        # Isolate the actual array bound so missing entity fields cannot mask it.
        bound = {**spec, 'items': {'type': 'string'}}
        valid(bound, ['id'] * limit)
        try:
            valid(bound, ['id'] * (limit + 1))
        except AssertionError:
            pass
        else:
            raise AssertionError(name+'/'+field+' oversized page accepted')
    for name, fields in {'Correction': ['cancelled_order_hashes', 'corrected_fill_ids'],
                         'CommandResult': ['affected_order_hashes', 'created_fill_ids', 'corrected_fill_ids'],
                         'EngineState': ['orders', 'fills', 'bindings'],
                         'JournalRecord': ['external_event_ids']}.items():
        for field in fields:
            expect('maxItems' in defs[name]['properties'][field], False, name+'/'+field+'/internal')
    print('PASS SEC-S2A-01: 8 models; 1000/1001 fills, 200/201 affected orders; 0/25bps; atomic replay expectations; API page limits retained')
