package dev.voiceos.client

import java.util.Locale

object AttachmentInput {
    const val MAX_BYTES = 5 * 1024 * 1024

    fun acceptedMediaType(filename: String, declaredType: String?): String? {
        val extension = filename.substringAfterLast('.', "").lowercase(Locale.US)
        val expected = when (extension) {
            "jpg", "jpeg" -> "image/jpeg"
            "png" -> "image/png"
            "webp" -> "image/webp"
            else -> return null
        }
        return expected.takeIf { declaredType.isNullOrBlank() || declaredType.substringBefore(';').trim().lowercase(Locale.US) == it }
    }

    fun requireUploadSize(byteCount: Int) {
        require(byteCount in 1..MAX_BYTES) { "Images must be between 1 byte and 5 MB." }
    }
}
