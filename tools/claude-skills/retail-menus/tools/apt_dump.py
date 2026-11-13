"""Dump an exported menu movie's actions as readable text (constant pool indices resolved, nested functions).

Usage: python apt_dump.py <movies/.../name.json> [block-offset] > out.txt
Reads the JSON setup writes (assets/private/menu-movies/movies/...). Opcode names are the SWF ones where they match;
EA's extended opcodes print as numbers with their operand. Integer operands of the push-constant opcodes print the
pool string in brackets. A reading aid only: never commit its output (game content).
"""
import json
import sys

NAMES = {
    0x00: 'end', 0x07: 'stop', 0x12: 'not', 0x17: 'pop', 0x1C: 'getVariable', 0x1D: 'setVariable',
    0x3D: 'callFunction', 0x3E: 'return', 0x40: 'newObject', 0x41: 'defineLocal', 0x43: 'initObject',
    0x47: 'add2', 0x48: 'less2', 0x49: 'equals2', 0x4E: 'getMember', 0x4F: 'setMember', 0x52: 'callMethod',
    0x53: 'newMethod', 0x87: 'storeRegister', 0x88: 'constantPool', 0x8E: 'defineFunction2', 0x96: 'push',
    0x99: 'jump', 0x9B: 'defineFunction', 0x9D: 'if', 0x4C: 'pushDuplicate', 0x67: 'greater', 0x0B: 'subtract',
    0x0C: 'multiply', 0x0D: 'divide', 0x54: 'instanceOf', 0x69: 'extends',
}


def dump(ops, pool, depth, out):
    pad = '  ' * depth
    for o in ops:
        code = o['opcode']
        name = NAMES.get(code, str(code))
        if code == 0x88:
            pool[:] = [v.get('value') for v in o.get('values', [])]
            out.append(f'{pad}{o["offset"]}: constantPool ({len(pool)})')
            continue
        text = ''
        if 'operand' in o:
            v = o['operand']
            text = repr(v)
            if isinstance(v, int) and 0 <= v < len(pool) and code in (162, 175, 176):
                text += f' [{pool[v]}]'
        if 'values' in o:
            text += ' ' + json.dumps([x.get('value') for x in o['values']])[:200]
        if code in (0x8E, 0x9B):
            params = ','.join(p.get('name', '') for p in o.get('parameters', []))
            out.append(f'{pad}{o["offset"]}: {name} {o.get("name", "")}({params})')
            dump(o.get('body', []), list(pool), depth + 1, out)
            continue
        out.append(f'{pad}{o["offset"]}: {name} {text}'.rstrip())


def main():
    movie = json.load(open(sys.argv[1], encoding='utf-8'))
    blocks = movie['actions']
    keys = [sys.argv[2]] if len(sys.argv) > 2 else list(blocks)
    out = []
    for key in keys:
        out.append(f'BLOCK {key}')
        dump(blocks[key], [], 1, out)
    sys.stdout.write('\n'.join(out) + '\n')


if __name__ == '__main__':
    main()
