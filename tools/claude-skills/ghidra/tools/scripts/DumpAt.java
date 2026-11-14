// Prints the bytes, the 32-bit big-endian words and the C string at each address (args: <out> <addr> [len] ...).
// An argument starting with '+' is the byte count for the addresses after it (default 64). Used by dump.ps1.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import java.io.PrintWriter;

public class DumpAt extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        int len = 64;
        try (PrintWriter out = new PrintWriter(args[0])) {
            for (int i = 1; i < args.length; i++) {
                if (args[i].startsWith("+")) { len = Integer.parseInt(args[i].substring(1)); continue; }
                Address a = toAddr(args[i].replaceFirst("^(0x|sub_)", ""));
                byte[] b = new byte[len];
                int n = currentProgram.getMemory().getBytes(a, b);
                out.println("// ==== " + a);
                StringBuilder hex = new StringBuilder(), words = new StringBuilder(), str = new StringBuilder();
                for (int k = 0; k < n; k++) hex.append(String.format("%02X%s", b[k], k % 16 == 15 ? "\n" : " "));
                for (int k = 0; k + 3 < n; k += 4)
                    words.append(String.format("%08X ", ((b[k] & 0xFF) << 24) | ((b[k + 1] & 0xFF) << 16) | ((b[k + 2] & 0xFF) << 8) | (b[k + 3] & 0xFF)));
                for (int k = 0; k < n && b[k] != 0; k++) str.append(b[k] >= 32 && b[k] < 127 ? (char) b[k] : '.');
                out.println(hex.toString().trim());
                out.println("words: " + words.toString().trim());
                out.println("string: \"" + str + "\"");
            }
        }
    }
}
