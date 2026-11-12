// Names decoded Skate 3 (TU3) functions in a Ghidra program from skate3-tu3-functions.tsv:
// address<TAB>name<TAB>comment[<TAB>source], '#' lines skipped. Sets the name (USER_DEFINED) and a plate comment,
// creating the function when none starts at the address. Skips and logs: non-code addresses, addresses inside
// another function, and functions that already carry a different user name.
// Script Manager: run it and pick the TSV (the log goes next to it). Headless (without -readOnly, so the
// names are saved): -postScript ApplyNames.java <tsv> <log file>
//@category Skate3
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.SourceType;

public class ApplyNames extends GhidraScript {
    @Override
    protected void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 2) {
            if (isRunningHeadless()) {
                printerr("usage: ApplyNames.java <tsv> <log file>");
                return;
            }
            String tsv = askFile("Skate 3 function names (TSV)", "Apply").getAbsolutePath();
            args = new String[] {tsv, tsv + ".log"};
        }
        int applied = 0, unchanged = 0, created = 0, skipped = 0;
        try (PrintWriter log = new PrintWriter(args[1], "UTF-8")) {
            for (String line : Files.readAllLines(Paths.get(args[0]), StandardCharsets.UTF_8)) {
                if (line.isBlank() || line.startsWith("#")) {
                    continue;
                }
                String[] f = line.split("\t");
                if (f.length < 2) {
                    log.println("SKIP bad line: " + line);
                    skipped++;
                    continue;
                }
                String text = f[0].trim().replaceFirst("(?i)^(sub_|0x)", "");
                String name = f[1].trim();
                String comment = f.length > 2 ? f[2].trim() : "";
                String source = f.length > 3 ? f[3].trim() : "";
                Address at = toAddr(Long.parseLong(text, 16));
                MemoryBlock block = getMemoryBlock(at);
                if (block == null || !block.isExecute()) {
                    log.println("SKIP " + text + " " + name + ": not in executable memory");
                    skipped++;
                    continue;
                }
                Function fn = getFunctionAt(at);
                if (fn == null) {
                    Function outer = getFunctionContaining(at);
                    if (outer != null) {
                        log.println("SKIP " + text + " " + name + ": inside " + outer.getName() + " @ " + outer.getEntryPoint());
                        skipped++;
                        continue;
                    }
                    fn = createFunction(at, null);
                    if (fn == null) {
                        log.println("SKIP " + text + " " + name + ": could not create a function");
                        skipped++;
                        continue;
                    }
                    created++;
                }
                if (fn.getName().equals(name)) {
                    unchanged++;
                }
                else if (fn.getSymbol().getSource() == SourceType.USER_DEFINED) {
                    log.println("SKIP " + text + " " + name + ": already named " + fn.getName());
                    skipped++;
                    continue;
                }
                else {
                    try {
                        fn.setName(name, SourceType.USER_DEFINED);
                        applied++;
                    }
                    catch (Exception e) {
                        log.println("SKIP " + text + " " + name + ": " + e.getMessage());
                        skipped++;
                        continue;
                    }
                }
                String plate = name + (comment.isEmpty() ? "" : ": " + comment) + (source.isEmpty() ? "" : " [" + source + "]");
                setPlateComment(at, plate);
                log.println("OK   " + text + " " + name);
            }
            String summary = "applied " + applied + ", unchanged " + unchanged + ", created " + created + ", skipped " + skipped;
            log.println(summary);
            println(summary);
        }
    }
}
