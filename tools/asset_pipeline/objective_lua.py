"""Translate retail challenge objective predicates (Lua 5.1 text) into the data form the engine evaluates.

The port never runs retail Lua (docs/hails-additions/42-challenges.md "Decision: objectives as data, not Lua").
This parses the subset the retail rows use (research b867) into a JSON tree matching
crates/skate-core/src/challenges/predicate.rs. Names stay strings; the game resolves them when it loads, and an
API name it does not know becomes a call that errors (retail's `DidSpecificTrickgroup` typo row), so the
condition gives its default as retail's protected call does (research b869).
"""
import re

FORMAT = 'skate3rust.objective-program'
VERSION = 1

# Enum tables the scripts read (research b869); others stay named for the game to resolve.
ENUMS = {
    'Flow.eObjectiveStatus.Disabled': 1, 'Flow.eObjectiveStatus.Enabled': 2, 'Flow.eObjectiveStatus.Active': 4,
    'Flow.eObjectiveStatus.Complete': 8, 'Flow.eObjectiveStatus.Failed': 16,
}

TOKEN = re.compile(r'''
    (?P<space>\s+)
  | (?P<comment>--\[(?P<eq>=*)\[.*?\](?P=eq)\]|--[^\n]*)
  | (?P<number>0[xX][0-9a-fA-F]+|\d+\.?\d*(?:[eE][+-]?\d+)?|\.\d+(?:[eE][+-]?\d+)?)
  | (?P<string>"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')
  | (?P<name>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<op>==|~=|<=|>=|\.\.\.|\.\.|[-+*/%^#<>=(){}\[\];:,.])
''', re.S | re.X)

KEYWORDS = {'and', 'break', 'do', 'else', 'elseif', 'end', 'false', 'for', 'function', 'if', 'in', 'local', 'nil',
            'not', 'or', 'repeat', 'return', 'then', 'true', 'until', 'while'}
COMPARE = {'==': 'eq', '~=': 'ne', '<': 'lt', '<=': 'le', '>': 'gt', '>=': 'ge'}
ESCAPES = {'n': '\n', 't': '\t', 'r': '\r', '\\': '\\', '"': '"', "'": "'", '\n': '\n', 'a': '\a', 'b': '\b',
           'f': '\f', 'v': '\v'}


class LuaSyntaxError(Exception):
    pass


def tokens(text):
    out, pos = [], 0
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            raise LuaSyntaxError(f'unexpected character at {pos}')
        pos = m.end()
        if m.group('space') is not None or m.group('comment') is not None:
            continue
        kind = next(k for k in ('number', 'string', 'name', 'op') if m.group(k) is not None)
        value = m.group(kind)
        if kind == 'name' and value in KEYWORDS:
            kind = 'keyword'
        out.append((kind, value))
    out.append(('eof', ''))
    return out


def unquote(raw):
    body, out, i = raw[1:-1], [], 0
    while i < len(body):
        c = body[i]
        if c == '\\' and i + 1 < len(body):
            n = body[i + 1]
            if n.isdigit():
                digits = re.match(r'\d{1,3}', body[i + 1:]).group(0)
                out.append(chr(int(digits)))
                i += 1 + len(digits)
                continue
            out.append(ESCAPES.get(n, n))
            i += 2
            continue
        out.append(c)
        i += 1
    return ''.join(out)


def number(raw):
    value = float(int(raw, 16)) if raw[:2].lower() == '0x' else float(raw)
    return int(value) if value.is_integer() and abs(value) < 2 ** 53 else value


class Parser:
    def __init__(self, text):
        self.toks = tokens(text)
        self.i = 0
        self.scopes = [{}]
        self.locals = 0

    # -- token helpers
    def peek(self, offset=0):
        return self.toks[self.i + offset]

    def take(self):
        tok = self.toks[self.i]
        self.i += 1
        return tok

    def accept(self, value):
        if self.peek()[1] == value and self.peek()[0] in ('op', 'keyword'):
            self.i += 1
            return True
        return False

    def expect(self, value):
        if not self.accept(value):
            raise LuaSyntaxError(f'expected {value!r}, found {self.peek()[1]!r}')

    def name(self):
        kind, value = self.take()
        if kind != 'name':
            raise LuaSyntaxError(f'expected a name, found {value!r}')
        return value

    # -- scopes
    def declare(self, name):
        index = self.locals
        self.locals += 1
        self.scopes[-1][name] = index
        return index

    def lookup(self, name):
        for scope in reversed(self.scopes):
            if name in scope:
                return scope[name]
        return None

    def block(self, ends):
        self.scopes.append({})
        body = []
        while self.peek()[1] not in ends or self.peek()[0] not in ('keyword', 'eof'):
            if self.peek()[0] == 'eof':
                if 'eof' in ends:
                    break
                raise LuaSyntaxError('unexpected end of script')
            stmt = self.statement()
            if isinstance(stmt, list):
                body.extend(stmt)
            elif stmt is not None:
                body.append(stmt)
        self.scopes.pop()
        return body

    # -- statements
    def statement(self):
        kind, value = self.peek()
        if self.accept(';'):
            return None
        if self.accept('local'):
            if self.peek()[1] == 'function':
                raise LuaSyntaxError('function definitions are outside the subset')
            target = self.name()
            init = self.expression() if self.accept('=') else {'lit': None}
            return {'set_local': self.declare(target), 'value': init}
        if self.accept('if'):
            arms = []
            test = self.expression()
            self.expect('then')
            arms.append([test, self.block({'elseif', 'else', 'end'})])
            otherwise = []
            while True:
                if self.accept('elseif'):
                    test = self.expression()
                    self.expect('then')
                    arms.append([test, self.block({'elseif', 'else', 'end'})])
                elif self.accept('else'):
                    otherwise = self.block({'end'})
                    self.expect('end')
                    break
                else:
                    self.expect('end')
                    break
            return {'if': arms, 'else': otherwise}
        if self.accept('for'):
            var = self.name()
            self.expect('=')
            start = self.expression()
            self.expect(',')
            limit = self.expression()
            step = self.expression() if self.accept(',') else {'lit': 1}
            self.expect('do')
            self.scopes.append({})
            index = self.declare(var)
            body = self.block({'end'})
            self.scopes.pop()
            self.expect('end')
            return {'for': index, 'start': start, 'limit': limit, 'step': step, 'body': body}
        if self.accept('return'):
            if self.peek()[1] in ('end', 'else', 'elseif', '') or self.peek()[0] == 'eof' or self.peek()[1] == ';':
                return {'return': {'lit': None}}
            return {'return': self.expression()}
        if self.accept('do'):
            body = self.block({'end'})
            self.expect('end')
            return body
        if kind == 'keyword':
            raise LuaSyntaxError(f'{value!r} is outside the subset')
        target = self.suffixed()
        if self.accept('='):
            return self.assign(target, self.expression())
        if 'api' in target or 'missing' in target or 'init_var' in target:
            return target if 'init_var' in target else {'call': target}
        raise LuaSyntaxError('expression used as a statement')

    def assign(self, target, value):
        if 'local' in target:
            return {'set_local': target['local'], 'value': value}
        if 'var' in target:
            return {'set_var': target['var'], 'value': value}
        if 'global' in target:
            return {'set_global': target['global'], 'value': value}
        # Assigning into anything else (a field of a function, e.g. retail's `InitVars.x = ...`) errors in Lua.
        return {'call': {'missing': 'assignment to a field'}}

    # -- expressions, lowest precedence first
    def expression(self):
        left = self.conjunction()
        while self.accept('or'):
            left = {'or': [left, self.conjunction()]}
        return left

    def conjunction(self):
        left = self.comparison()
        while self.accept('and'):
            left = {'and': [left, self.comparison()]}
        return left

    def comparison(self):
        left = self.additive()
        while self.peek()[0] == 'op' and self.peek()[1] in COMPARE:
            op = COMPARE[self.take()[1]]
            left = {'cmp': op, 'args': [left, self.additive()]}
        return left

    def additive(self):
        left = self.unary()
        while self.peek()[0] == 'op' and self.peek()[1] in ('+', '-'):
            op = 'add' if self.take()[1] == '+' else 'sub'
            left = {'arith': op, 'args': [left, self.unary()]}
        return left

    def unary(self):
        if self.accept('not'):
            return {'not': self.unary()}
        if self.peek() == ('op', '-'):
            self.take()
            return {'neg': self.unary()}
        if self.peek()[0] == 'op' and self.peek()[1] in ('*', '/', '%', '^', '..', '#'):
            raise LuaSyntaxError(f'{self.peek()[1]!r} is outside the subset')
        return self.suffixed()

    def suffixed(self):
        kind, value = self.take()
        if kind == 'number':
            return {'lit': number(value)}
        if kind == 'string':
            return {'lit': unquote(value)}
        if value in ('nil', 'true', 'false') and kind == 'keyword':
            return {'lit': {'nil': None, 'true': True, 'false': False}[value]}
        if value == '(' and kind == 'op':
            inner = self.expression()
            self.expect(')')
            return self.index_chain(inner)
        if value == '{' and kind == 'op':
            items = []
            while not self.accept('}'):
                items.append(self.expression())
                if not (self.accept(',') or self.accept(';')):
                    self.expect('}')
                    break
            return self.index_chain({'table': items})
        if kind != 'name':
            raise LuaSyntaxError(f'unexpected {value!r}')
        local = self.lookup(value)
        if local is not None:
            return self.index_chain({'local': local})
        if value == 'vars' and self.peek()[1] in ('.', '['):
            key = self.field()
            return self.index_chain({'var': key})
        dotted = [value]
        while self.peek() == ('op', '.') and self.peek(1)[0] == 'name':
            self.take()
            dotted.append(self.take()[1])
        path = '.'.join(dotted)
        if self.peek() == ('op', '('):
            args = self.arguments()
            if path == 'InitVars' and len(args) == 3 and args[0] == {'vars': True} and isinstance(args[1].get('lit'), str):
                return {'init_var': args[1]['lit'], 'value': args[2]}
            return self.index_chain({'api': path, 'args': args})
        if path in ENUMS:
            return {'lit': ENUMS[path]}
        if len(dotted) > 1:
            return self.index_chain({'enum': path})
        if value == 'vars':
            return {'vars': True}
        return self.index_chain({'global': value})

    def field(self):
        if self.accept('.'):
            return self.name()
        self.expect('[')
        key = self.expression()
        self.expect(']')
        if not isinstance(key.get('lit'), str):
            raise LuaSyntaxError('vars keys must be string literals')
        return key['lit']

    def arguments(self):
        self.expect('(')
        args = []
        if not self.accept(')'):
            args.append(self.expression())
            while self.accept(','):
                args.append(self.expression())
            self.expect(')')
        return args

    def index_chain(self, expr):
        while self.peek() in (('op', '['), ('op', '.')):
            if self.take()[1] == '.':
                key = {'lit': self.name()}
            else:
                key = self.expression()
                self.expect(']')
            expr = {'index': [expr, key]}
        if self.peek() == ('op', ':'):
            raise LuaSyntaxError('method calls are outside the subset')
        return expr


def translate(text):
    """One condition's text as a program; a syntax error becomes a program that errors when run (retail never
    compiles it and returns the condition's default every time, research b869)."""
    try:
        parser = Parser(text or '')
        body = parser.block({'eof'})
        return {'locals': parser.locals, 'body': body}
    except LuaSyntaxError as e:
        return {'locals': 0, 'body': [{'call': {'missing': f'syntax error: {e}'}}], 'syntax_error': str(e)}
