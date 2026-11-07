"""Translate retail progression graph handlers (compiled Lua 5.1 chunks) into the career op data (research b889 op
schema; design b887 step 7; doc 43). The port never runs retail Lua: each handler becomes a short op list. Any call or
construct the schema does not cover raises TranslateError, so a new bank or DLC never yields a silently wrong graph.

Chunk format (b889): `\\x1bLuaQ`, little-endian, int / size_t / instruction 4 bytes, 4-byte float numbers; the handler
body is the main function's first proto (5 params; register 1 the message, 2 the graph instance table, 4 the career
flags table)."""
import struct


class TranslateError(ValueError):
    pass


OPS = ('MOVE LOADK LOADBOOL LOADNIL GETUPVAL GETGLOBAL GETTABLE SETGLOBAL SETUPVAL SETTABLE NEWTABLE SELF ADD SUB '
       'MUL DIV MOD POW UNM NOT LEN CONCAT JMP EQ LT LE TEST TESTSET CALL TAILCALL RETURN FORLOOP FORPREP TFORLOOP '
       'SETLIST CLOSE CLOSURE VARARG').split()


def parse_chunk(data):
    """The main function of a Lua 5.1 chunk as {nparams, code, k, protos}."""
    if data[:5] != b'\x1bLuaQ':
        raise TranslateError('not a Lua 5.1 chunk')
    if data[6] != 1 or data[8] != 4 or data[10] != 4:
        raise TranslateError('unexpected chunk layout')
    pos = 12

    def u8():
        nonlocal pos
        pos += 1
        return data[pos - 1]

    def i32():
        nonlocal pos
        pos += 4
        return struct.unpack_from('<i', data, pos - 4)[0]

    def text():
        nonlocal pos
        n = i32()
        pos += n
        return data[pos - n:pos - 1].decode('latin1') if n else None

    def func():
        nonlocal pos
        text()
        i32(), i32()
        nups, nparams, _, _ = u8(), u8(), u8(), u8()
        n = i32()
        code = list(struct.unpack_from('<%dI' % n, data, pos))
        pos += 4 * n
        k = []
        for _ in range(i32()):
            t = u8()
            if t == 0:
                k.append(None)
            elif t == 1:
                k.append(bool(u8()))
            elif t == 3:
                k.append(struct.unpack_from('<f', data, pos)[0])
                pos += 4
            elif t == 4:
                k.append(text())
            else:
                raise TranslateError('constant type %d' % t)
        protos = [func() for _ in range(i32())]
        n = i32()
        pos += 4 * n
        for _ in range(i32()):
            text(), i32(), i32()
        for _ in range(i32()):
            text()
        return {'nups': nups, 'nparams': nparams, 'code': code, 'k': k, 'protos': protos}

    return func()


# Calls that become ops: (function path) -> (op name, argument names); `None` args = no arguments.
CALLS = {
    'Actions.ActivateAction': ('arm_action', ['action']),
    'Unlock.UnlockProgressionReward': ('unlock_reward', ['reward', 'flag']),
    'ChallengeStatus.ActivateChallenge': ('activate_challenge', ['challenge', 'flag']),
    'ChallengeStatus.DeactivateChallenge': ('deactivate_challenge', ['challenge', 'flag']),
    'ChallengeStatus.ActivateChallengeGroup': ('activate_group', ['group', 'flag']),
    'TeamBrand.UnlockTier': ('brand_tier', ['tier']),
    'Achievement.GrantAchievement': ('grant_achievement', ['name']),
    'SkateFeed.SendMessage': ('skatefeed', ['text']),
    'SignUp.SetAutoSignUp': ('auto_sign_up', ['challenge']),
    'Unlock.UnlockTeammate': ('unlock_teammate', []),
    'Unlock.MilestoneEarned': ('milestone_earned', []),
    'Discovery.AllowDiscovery': ('allow_discovery', []),
    'Progression.HideScreen': ('hide_screen', []),
    'Progression.UnhideScreen': ('unhide_screen', []),
    'Notifications101.Show101': ('show_101', ['title', 'description']),
    'Popup.ShowYesNoPopup': ('show_yes_no', ['title', 'description']),
    'TeamBrand.LoadChallengeCareerPhoto': ('load_career_photo', ['challenge']),
    'TeamBrand.UnloadChallengeCareerPhoto': ('unload_career_photo', ['challenge']),
}
REPLIES = {'show_101': 'hudcomplete', 'show_yes_no': ['hudyes', 'hudno'],
           'load_career_photo': 'challengeteambrandimagecomplete'}
STATE_CHANGES = {'ChangeState': 'none', 'Events.ExecuteEventsAndChangeState': 'execute',
                 'Events.DeferEventsAndChangeState': 'defer'}
# Calls whose result is only used as a value by later code.
QUERIES = {'Attrib.RefSpec', 'ChallengeStatus.GetChallengeCompletionLevel'}
# Handler parameters (registers): the message, the graph instance's own table, the shared career flags table.
MESSAGE, INSTANCE, FLAGS = ('arg', 1), ('arg', 2), ('arg', 4)


def _path(v):
    """'Lib.Function' for a global or a global's field chain."""
    if v[0] == 'g':
        return v[1]
    if v[0] == 'field' and v[2][0] == 'k':
        base = _path(v[1])
        return base + '.' + str(v[2][1]) if base else None
    return None


def _value(v):
    """An op argument: a constant, the picked challenge, or a challenge named through Attrib.RefSpec(...).Name."""
    if v[0] == 'k':
        return v[1]
    if v[0] == 'pick':
        for branch, val in v[1]:
            if val and val[0] == 'pick':
                _value(val)
            elif val and val[0] == 'k' and isinstance(val[1], str) and not any(o.get('op') == 'pick' for o in branch):
                branch.append({'op': 'pick', 'challenge': val[1]})
        return '$picked'
    if v[0] == 'field' and v[2] == ('k', 'Name') and v[1][0] == 'call' and v[1][1] == 'Attrib.RefSpec':
        return _value(v[1][2][-1])
    raise TranslateError(f'argument {v!r} is not a constant')


def _challenge_of(v):
    """The challenge of Attrib.RefSpec(challenges, X).GlobalData.<field>."""
    if v[0] == 'field' and v[1][0] == 'field' and v[1][2] == ('k', 'GlobalData'):
        ref = v[1][1]
        if ref[0] == 'call' and ref[1] == 'Attrib.RefSpec':
            return _value(ref[2][-1])
    if v[0] == 'field' and v[1][0] == 'field' and v[1][1][0] == 'field' and v[1][1][2] == ('k', 'GlobalData'):
        return _challenge_of(v[1])
    raise TranslateError(f'teleport target {v!r}')


def _condition(left, right):
    for a, b in ((left, right), (right, left)):
        # The school exit (shape 2b): after the flag chain, "no challenge was picked".
        if a[0] == 'pick' and b in (('k', None), ('nil',)):
            _value(a)
            return {'picked_none': True}
        if a[0] == 'field' and a[1] == FLAGS and a[2][0] == 'k' and b == ('k', False):
            return {'flag_false': a[2][1]}
        if (a[0] == 'field' and a[1] == MESSAGE and a[2] == ('k', 'RequestID')
                and b[0] == 'field' and b[1] == INSTANCE and b[2] == ('k', 'RequestID')):
            return {'reply_matches': 'RequestID'}
        if a[0] == 'call' and a[1] == 'ChallengeStatus.GetChallengeCompletionLevel' and _path(b):
            level = _path(b).rsplit('.', 1)[-1].lower()
            return {'completion_is': {'challenge': _value(a[2][0]), 'level': level}}
    raise TranslateError(f'condition {left!r} == {right!r}')


class _Walk:
    def __init__(self, proto):
        self.code, self.k = proto['code'], proto['k']
        self.np = proto['nparams']

    def rk(self, regs, v):
        return ('k', self.k[v - 256]) if v >= 256 else regs.get(v, ('nil',))

    def decode(self, pc):
        ins = self.code[pc]
        op = OPS[ins & 63] if (ins & 63) < len(OPS) else None
        return op, (ins >> 6) & 255, ins >> 23, (ins >> 14) & 511, ins >> 14, (ins >> 14) - 131071

    def block(self, start, end, regs):
        ops, pc = [], start
        while pc < end:
            op, a, b, c, bx, sbx = self.decode(pc)
            if op == 'MOVE':
                regs[a] = regs.get(b, ('nil',))
            elif op == 'LOADK':
                regs[a] = ('k', self.k[bx])
            elif op == 'LOADBOOL':
                regs[a] = ('k', bool(b))
                if c:
                    pc += 1
            elif op == 'LOADNIL':
                for r in range(a, b + 1):
                    regs[r] = ('k', None)
            elif op == 'GETGLOBAL':
                regs[a] = ('g', self.k[bx])
            elif op == 'GETTABLE':
                regs[a] = ('field', regs.get(b, ('nil',)), self.rk(regs, c))
            elif op == 'SETTABLE':
                ops.extend(self.store(regs.get(a, ('nil',)), self.rk(regs, b), self.rk(regs, c), ops))
            elif op == 'CALL':
                if b == 0:
                    raise TranslateError('variable argument call')
                fn = _path(regs.get(a, ('nil',)))
                args = [regs.get(a + 1 + i, ('nil',)) for i in range(b - 1)]
                ops.extend(self.call(fn, args))
                if c != 1:
                    regs[a] = ('call', fn, args)
            elif op in ('EQ', 'TEST'):
                op2 = self.decode(pc + 1)
                if op2[0] != 'JMP':
                    raise TranslateError(f'{op} not followed by JMP at {pc}')
                target = pc + 2 + op2[5]
                if op == 'EQ':
                    cond = _condition(self.rk(regs, b), self.rk(regs, c))
                    negate = a == 1
                else:
                    raise TranslateError(f'TEST at {pc}')
                then_end, else_end = target, target
                if target > pc + 2 and self.decode(target - 1)[0] == 'JMP':
                    jump = target + self.decode(target - 1)[5]
                    if jump > target:
                        then_end, else_end = target - 1, jump
                then_regs, else_regs = dict(regs), dict(regs)
                then_ops = self.block(pc + 2, then_end, then_regs)
                else_ops = self.block(target, else_end, else_regs) if else_end > target else []
                # A register set to different values in the branches: if later code reads it, it is the picked
                # challenge (shape 2b) and each branch gets a `pick` op then (scratch registers stay unused).
                for r in set(then_regs) | set(else_regs):
                    tv, ev = then_regs.get(r), else_regs.get(r)
                    if tv != ev:
                        regs[r] = ('pick', [(then_ops, tv), (else_ops, ev)])
                    elif tv is not None:
                        regs[r] = tv
                if negate:
                    then_ops, else_ops = else_ops, then_ops
                ops.append({'op': 'if', 'cond': cond, 'then': then_ops, 'else': else_ops})
                pc = else_end
                continue
            elif op == 'JMP':
                pc = pc + 1 + sbx
                continue
            elif op == 'RETURN':
                break
            else:
                raise TranslateError(f'opcode {op} at {pc}')
            pc += 1
        return ops

    def store(self, table, key, value, ops):
        if table == FLAGS and key[0] == 'k' and value[0] == 'k' and isinstance(value[1], bool):
            return [{'op': 'set_flag', 'name': key[1], 'value': value[1]}]
        if table == INSTANCE and key == ('k', 'RequestID') and value[0] == 'call':
            for op in reversed(ops):
                if op['op'] == 'load_career_photo':
                    op['store'] = 'RequestID'
                    return []
        raise TranslateError(f'store {table!r}[{key!r}] = {value!r}')

    def call(self, fn, args):
        if fn in QUERIES:
            return []
        if fn in STATE_CHANGES:
            return [{'op': 'change_state', 'state': _value(args[0]), 'events': STATE_CHANGES[fn]}]
        if fn == 'Teleport.TeleportLocalToWorld':
            flag = _value(args[2]) if len(args) > 2 else None
            if args[0][0] == 'k':
                return [{'op': 'teleport', 'locator': args[0][1], 'district': _value(args[1]), 'flag': flag}]
            return [{'op': 'teleport_to_challenge', 'challenge': _challenge_of(args[0]), 'flag': flag}]
        if fn not in CALLS:
            raise TranslateError(f'call {fn}')
        name, fields = CALLS[fn]
        op = {'op': name}
        for i, field in enumerate(fields):
            op[field] = _value(args[i]) if i < len(args) else None
        if name in REPLIES:
            op['reply'] = REPLIES[name]
        return [op]


def translate_handler(proto):
    """The op list of one handler proto (the closure inside the handler's chunk)."""
    walk = _Walk(proto)
    regs = {i: ('arg', i) for i in range(proto['nparams'])}
    return walk.block(0, len(proto['code']), regs)


def translate_chunk(data):
    main = parse_chunk(data)
    if not main['protos']:
        raise TranslateError('chunk defines no handler function')
    return translate_handler(main['protos'][0])
