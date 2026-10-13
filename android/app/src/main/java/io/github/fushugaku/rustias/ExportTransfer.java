package io.github.fushugaku.rustias;

import java.io.Closeable;
import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.util.Base64;

/** A bounded, ordered export. A partial recording never reaches the document picker. */
final class ExportTransfer implements Closeable {
    static final int CHUNK_BYTES = 128 * 1024;
    static final long MAX_BYTES = 2L * 1024 * 1024 * 1024;
    final String name;
    final String mime;
    private final File file;
    private final FileOutputStream stream;
    private final long size;
    private long received;
    private boolean finished;

    ExportTransfer(File directory, String name, String mime, long size) throws IOException {
        if (size < 0 || size > MAX_BYTES) throw new IOException("Export exceeds the 2 GB file limit.");
        this.name = safeName(name);
        this.mime = mime.matches("[a-zA-Z0-9.+_-]+/[a-zA-Z0-9.+_-]+") ? mime : "application/octet-stream";
        this.size = size;
        file = File.createTempFile("rustias-export-", ".tmp", directory);
        stream = new FileOutputStream(file);
    }

    long append(long offset, String encoded) throws IOException {
        if (finished || offset != received) throw new IOException("Export chunks arrived out of order.");
        if (encoded.length() > 4 * ((CHUNK_BYTES + 2) / 3)) throw new IOException("Export chunk is too large.");
        final byte[] bytes;
        try { bytes = Base64.getDecoder().decode(encoded); }
        catch (IllegalArgumentException error) { throw new IOException("Invalid export chunk.", error); }
        if (bytes.length == 0 || bytes.length > CHUNK_BYTES || received + bytes.length > size) {
            throw new IOException("Export chunk does not match the file size.");
        }
        stream.write(bytes);
        received += bytes.length;
        return received;
    }

    File finish() throws IOException {
        if (finished || received != size) throw new IOException("Export file is incomplete.");
        stream.getFD().sync();
        stream.close();
        finished = true;
        return file;
    }

    @Override public void close() throws IOException {
        try { stream.close(); }
        finally { if (file.exists() && !file.delete()) file.deleteOnExit(); }
    }

    static String safeName(String name) {
        String clean = name.replaceAll("[\\\\/\\p{Cntrl}]", "_").trim();
        if (clean.isEmpty() || clean.equals(".") || clean.equals("..")) return "rustias-export";
        if (clean.length() <= 120) return clean;
        int dot = clean.lastIndexOf('.');
        String extension = dot > 0 && clean.length() - dot <= 10 ? clean.substring(dot) : "";
        return clean.substring(0, 120 - extension.length()) + extension;
    }
}
