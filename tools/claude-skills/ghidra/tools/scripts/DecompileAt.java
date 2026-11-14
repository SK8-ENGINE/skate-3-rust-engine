// Prints Ghidra's pseudocode for the functions at the given addresses (hex, with or without 0x / sub_), for reading
// TU3 code without its assembly (skill ghidra). Reference only: never copy the output into the repo.
// Headless: -postScript DecompileAt.java <out file> <address> [address ...]
//@category Skate3
import java.io.PrintWriter;

import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class DecompileAt extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            printerr("usage: DecompileAt.java <out file> <address> [address ...]");
            return;
        }
        DecompInterface decompiler = new DecompInterface();
        decompiler.openProgram(currentProgram);
        try (PrintWriter out = new PrintWriter(args[0], "UTF-8")) {
            for (int i = 1; i < args.length; i++) {
                String text = args[i].replaceFirst("(?i)^(sub_|0x)", "");
                Address at = toAddr(Long.parseLong(text, 16));
                Function f = getFunctionContaining(at);
                if (f == null) {
                    // Not found by analysis: make one at the address (the recomp's sub_ names are function starts).
                    f = createFunction(at, null);
                }
                out.println("// ==== " + args[i] + (f == null ? " : no function" : " : " + f.getName() + " @ " + f.getEntryPoint()));
                if (f == null) {
                    continue;
                }
                DecompileResults r = decompiler.decompileFunction(f, 120, monitor);
                out.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// decompile failed: " + r.getErrorMessage());
            }
        }
        finally {
            decompiler.dispose();
        }
    }
}
