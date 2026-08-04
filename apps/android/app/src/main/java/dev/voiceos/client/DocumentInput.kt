package dev.voiceos.client

import android.content.ContentResolver
import android.net.Uri
import android.provider.OpenableColumns
import java.io.ByteArrayOutputStream
import java.util.Locale

object DocumentInput {
    private const val MAX_BYTES = 5 * 1024 * 1024

    fun readBytes(contentResolver: ContentResolver, uri: Uri): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(8_192)
        contentResolver.openInputStream(uri)?.use { input ->
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                if (output.size() + count > MAX_BYTES) {
                    throw IllegalArgumentException("Files must be 5 MB or smaller.")
                }
                output.write(buffer, 0, count)
            }
        } ?: throw IllegalArgumentException("The selected file could not be opened.")
        if (output.size() == 0) throw IllegalArgumentException("The selected file is empty.")
        return output.toByteArray()
    }

    fun filename(contentResolver: ContentResolver, uri: Uri): String {
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val index = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0) return cursor.getString(index) ?: "document.txt"
            }
        }
        return uri.lastPathSegment?.substringAfterLast('/') ?: "document.txt"
    }

    fun mediaTypeForFilename(filename: String): String = when (
        filename.substringAfterLast('.', "").lowercase(Locale.US)
    ) {
        "md", "markdown" -> "text/markdown"
        "csv" -> "text/csv"
        "json" -> "application/json"
        else -> "text/plain"
    }
}
