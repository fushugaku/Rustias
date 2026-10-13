package io.github.fushugaku.rustias;

import org.junit.Test;
import org.junit.Rule;
import org.junit.rules.TemporaryFolder;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.util.Base64;
import static org.junit.Assert.*;

public class ExportTransferTest {
    @Rule public TemporaryFolder temporary = new TemporaryFolder();

    @Test public void recordingIsReassembledInOrderAndTemporaryFileIsRemoved() throws Exception {
        byte[] expected = new byte[ExportTransfer.CHUNK_BYTES * 2 + 37];
        for (int i = 0; i < expected.length; i++) expected[i] = (byte)(i * 31);
        File completed;
        try (ExportTransfer transfer = new ExportTransfer(temporary.getRoot(), "Program 1 — take.wav", "audio/wav", expected.length)) {
            for (int offset = 0; offset < expected.length; offset += ExportTransfer.CHUNK_BYTES) {
                byte[] part = java.util.Arrays.copyOfRange(expected, offset, Math.min(offset + ExportTransfer.CHUNK_BYTES, expected.length));
                assertEquals(offset + part.length, transfer.append(offset, Base64.getEncoder().encodeToString(part)));
            }
            completed = transfer.finish();
            assertArrayEquals(expected, Files.readAllBytes(completed.toPath()));
        }
        assertFalse(completed.exists());
    }

    @Test public void incompleteOrMisorderedFilesCannotBeExported() throws Exception {
        try (ExportTransfer transfer = new ExportTransfer(temporary.getRoot(), "patch.json", "application/json", 4)) {
            assertThrows(IOException.class, () -> transfer.append(1, "AQI="));
            assertEquals(2, transfer.append(0, "AQI="));
            assertThrows(IOException.class, transfer::finish);
            assertThrows(IOException.class, () -> transfer.append(2, "invalid*"));
            assertThrows(IOException.class, () -> transfer.append(2, "AwQF"));
            assertEquals(4, transfer.append(2, "AwQ="));
            assertArrayEquals(new byte[]{1,2,3,4}, Files.readAllBytes(transfer.finish().toPath()));
            assertThrows(IOException.class, () -> transfer.append(4, "BQ=="));
        }
    }

    @Test public void namesRemainFileNamesAndKeepTheFormat() {
        assertEquals(".._take_.wav", ExportTransfer.safeName("../take\n.wav"));
        assertEquals("rustias-export", ExportTransfer.safeName(".."));
        String longName = ExportTransfer.safeName("a".repeat(150) + ".mp3");
        assertEquals(120, longName.length());
        assertTrue(longName.endsWith(".mp3"));
    }

    @Test public void cancelCleansUpThePartialFile() throws Exception {
        ExportTransfer transfer = new ExportTransfer(temporary.getRoot(), "take.wav", "audio/wav", 10);
        transfer.append(0, "AQI=");
        transfer.close();
        assertEquals(0, temporary.getRoot().list().length);
    }
}
