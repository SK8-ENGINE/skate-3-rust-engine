import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import java.io.PrintWriter;

public class FindImm64 extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(args[0])) {
            for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                if (!b.isInitialized() || !b.isExecute()) continue;
                long n = b.getSize() / 4;
                byte[] buf = new byte[(int) b.getSize()];
                b.getBytes(b.getStart(), buf);
                for (int i = 1; i < args.length; i++) {
                    long v = Long.parseUnsignedLong(args[i], 16);
                    int h = (int) ((v >>> 16) & 0xFFFF), l = (int) (v & 0xFFFF);
                    for (int k = 0; k + 1 < n; k++) {
                        int w = word(buf, k);
                        int op = w >>> 26; boolean lis = op == 15 && ((w >>> 16) & 0x1F) == 0; int ha = (h + (l >= 0x8000 ? 1 : 0)) & 0xFFFF; boolean m1 = (op == 25 || lis) && (w & 0xFFFF) == h; boolean m2 = lis && (w & 0xFFFF) == ha; if (!m1 && !m2) continue;
                        for (int j = 1; j <= 3 && k + j < n; j++) {
                            int x = word(buf, k + j);
                            if ((m1 && (x >>> 26) == 24 && (x & 0xFFFF) == l) || (m2 && (x >>> 26) == 14 && (x & 0xFFFF) == l)) {
                                Address a = b.getStart().add(4L * k);
                                Function f = getFunctionContaining(a);
                                out.println(args[i] + " at " + a + (f != null ? " in " + f.getName() : ""));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    private static int word(byte[] b, int k) {
        int o = 4 * k;
        return ((b[o] & 0xFF) << 24) | ((b[o + 1] & 0xFF) << 16) | ((b[o + 2] & 0xFF) << 8) | (b[o + 3] & 0xFF);
    }
}
