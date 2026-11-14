// Finds each C string (exact, NUL-terminated) in the image and lists the functions that reference it
// (args: <out> <string> [string ...]). Used by find-strings.ps1.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;

public class FindStrings extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        try (PrintWriter out = new PrintWriter(args[0])) {
            for (int i = 1; i < args.length; i++) {
                byte[] s = args[i].getBytes(StandardCharsets.US_ASCII);
                byte[] needle = new byte[s.length + 1];
                System.arraycopy(s, 0, needle, 0, s.length);
                out.println("// ==== \"" + args[i] + "\"");
                Address from = currentProgram.getMinAddress();
                while (from != null) {
                    Address hit = currentProgram.getMemory().findBytes(from, needle, null, true, monitor);
                    if (hit == null) break;
                    byte[] before = new byte[1];
                    boolean start = currentProgram.getMemory().getBytes(hit.subtract(1), before) == 1 && before[0] == 0;
                    if (start) {
                        out.println("string at " + hit);
                        for (Reference r : getReferencesTo(hit)) {
                            Function f = getFunctionContaining(r.getFromAddress());
                            out.println("  ref from " + r.getFromAddress() + (f == null ? "" : " in " + f.getEntryPoint()));
                        }
                    }
                    from = hit.add(1);
                }
            }
        }
    }
}
