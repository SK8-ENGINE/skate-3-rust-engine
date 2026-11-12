// Where an address is referenced (args: <out> <addr> ...): Ghidra's references plus every place its 32-bit big-endian
// value sits in initialized memory (vtable slots the analysis did not mark), each with the words around it. Used by find-refs.ps1.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Reference;
import java.io.PrintWriter;

public class FindRefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(args[0])) {
            for (int i = 1; i < args.length; i++) {
                Address a = toAddr(args[i].replaceFirst("^(0x|sub_)", ""));
                out.println("// ==== " + a);
                for (Reference r : getReferencesTo(a)) {
                    Function f = getFunctionContaining(r.getFromAddress());
                    out.println("ref " + r.getFromAddress() + " " + r.getReferenceType() + (f != null ? " in " + f.getName() : ""));
                }
                long v = a.getOffset();
                byte[] pat = {(byte) (v >> 24), (byte) (v >> 16), (byte) (v >> 8), (byte) v};
                for (MemoryBlock b : currentProgram.getMemory().getBlocks()) {
                    if (!b.isInitialized()) continue;
                    Address at = b.getStart();
                    while (at != null && at.compareTo(b.getEnd()) <= 0) {
                        Address hit = currentProgram.getMemory().findBytes(at, b.getEnd(), pat, null, true, monitor);
                        if (hit == null) break;
                        if (hit.getOffset() % 4 == 0) {
                            StringBuilder w = new StringBuilder();
                            byte[] ctx = new byte[48];
                            int n = currentProgram.getMemory().getBytes(hit.subtract(16), ctx);
                            for (int k = 0; k + 3 < n; k += 4)
                                w.append(String.format("%08X ", ((ctx[k] & 0xFF) << 24) | ((ctx[k + 1] & 0xFF) << 16) | ((ctx[k + 2] & 0xFF) << 8) | (ctx[k + 3] & 0xFF)));
                            out.println("value at " + hit + " (" + b.getName() + "), words from -16: " + w.toString().trim());
                        }
                        at = hit.add(1);
                    }
                }
            }
        }
    }
}
