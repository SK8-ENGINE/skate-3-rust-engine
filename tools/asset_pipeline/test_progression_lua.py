import struct
import unittest

from tools.asset_pipeline.progression_lua import OPS, TranslateError, parse_chunk, translate_handler


def abc(op, a, b=0, c=0):
    return OPS.index(op) | a << 6 | c << 14 | b << 23


def abx(op, a, bx):
    return OPS.index(op) | a << 6 | bx << 14


def asbx(op, a, sbx):
    return abx(op, a, sbx + 131071)


class Proto:
    """A hand-built handler: 5 params, constants added on demand."""

    def __init__(self):
        self.code, self.k = [], []

    def const(self, v):
        if v not in self.k:
            self.k.append(v)
        return self.k.index(v)

    def rk(self, v):
        return 256 + self.const(v)

    def call(self, path, *args, reg=5):
        lib, _, fn = path.rpartition('.')
        if lib:
            self.code.append(abx('GETGLOBAL', reg, self.const(lib)))
            self.code.append(abc('GETTABLE', reg, reg, self.rk(fn)))
        else:
            self.code.append(abx('GETGLOBAL', reg, self.const(fn)))
        for i, arg in enumerate(args):
            if isinstance(arg, bool):
                self.code.append(abc('LOADBOOL', reg + 1 + i, int(arg), 0))
            else:
                self.code.append(abx('LOADK', reg + 1 + i, self.const(arg)))
        self.code.append(abc('CALL', reg, len(args) + 1, 1))

    def proto(self):
        return {'nparams': 5, 'code': self.code + [abc('RETURN', 0, 1)], 'k': self.k, 'protos': []}


class ProgressionLua(unittest.TestCase):
    def test_straight_line_calls_become_ops(self):
        p = Proto()
        p.call('Actions.ActivateAction', 'milestone_01_complete')
        p.call('Unlock.UnlockProgressionReward', 'pcu_training', True)
        p.call('Unlock.MilestoneEarned')
        p.call('Events.DeferEventsAndChangeState', 'complete')
        self.assertEqual(translate_handler(p.proto()), [
            {'op': 'arm_action', 'action': 'milestone_01_complete'},
            {'op': 'unlock_reward', 'reward': 'pcu_training', 'flag': True},
            {'op': 'milestone_earned'},
            {'op': 'change_state', 'state': 'complete', 'events': 'defer'}])

    def test_flag_writes_and_a_101_card_wait(self):
        p = Proto()
        p.code.append(abc('LOADBOOL', 5, 1, 0))
        p.code.append(abc('SETTABLE', 4, p.rk('CompletedBasicSchool'), 5))
        p.call('Notifications101.Show101', 'ID_T', 'ID_D')
        ops = translate_handler(p.proto())
        self.assertEqual(ops[0], {'op': 'set_flag', 'name': 'CompletedBasicSchool', 'value': True})
        self.assertEqual(ops[1]['reply'], 'hudcomplete')

    def test_an_if_on_a_flag_being_false(self):
        p = Proto()
        p.code.append(abc('GETTABLE', 5, 4, p.rk('Seen')))
        p.code.append(abc('EQ', 0, 5, p.rk(False)))
        jump = len(p.code)
        p.code.append(0)
        p.call('Popup.ShowYesNoPopup', 'ID_T', 'ID_D')
        p.code[jump] = asbx('JMP', 0, len(p.code) - jump - 1)
        ops = translate_handler(p.proto())
        self.assertEqual(ops, [{'op': 'if', 'cond': {'flag_false': 'Seen'}, 'else': [], 'then': [
            {'op': 'show_yes_no', 'title': 'ID_T', 'description': 'ID_D', 'reply': ['hudyes', 'hudno']}]}])

    def test_set_discovery_status_reads_the_enum_value(self):
        # Discovery.SetDiscoveryStatus("ots_dwgh_01", Discovery.eDiscoveryStatus.Discovered) (Danny Way pack, b912).
        p = Proto()
        p.code.append(abx('GETGLOBAL', 5, p.const('Discovery')))
        p.code.append(abc('GETTABLE', 5, 5, p.rk('SetDiscoveryStatus')))
        p.code.append(abx('LOADK', 6, p.const('ots_dwgh_01')))
        p.code.append(abx('GETGLOBAL', 7, p.const('Discovery')))
        p.code.append(abc('GETTABLE', 7, 7, p.rk('eDiscoveryStatus')))
        p.code.append(abc('GETTABLE', 7, 7, p.rk('Discovered')))
        p.code.append(abc('CALL', 5, 3, 1))
        self.assertEqual(translate_handler(p.proto()),
                         [{'op': 'set_discovery', 'challenge': 'ots_dwgh_01', 'status': 'Discovered'}])

    def test_unknown_calls_fail_the_export(self):
        p = Proto()
        p.call('Mystery.DoThing', 'x')
        with self.assertRaises(TranslateError):
            translate_handler(p.proto())

    def test_the_chunk_header_is_checked(self):
        with self.assertRaises(TranslateError):
            parse_chunk(b'not lua')
        header = b'\x1bLuaQ' + bytes([0, 1, 4, 4, 4, 4, 0])
        body = struct.pack('<i', 0) + struct.pack('<ii', 0, 0) + bytes([0, 0, 2, 2]) + struct.pack('<i', 1)
        body += struct.pack('<I', abc('RETURN', 0, 1)) + struct.pack('<i', 0) * 5
        self.assertEqual(parse_chunk(header + body)['code'], [abc('RETURN', 0, 1)])


if __name__ == '__main__':
    unittest.main()
