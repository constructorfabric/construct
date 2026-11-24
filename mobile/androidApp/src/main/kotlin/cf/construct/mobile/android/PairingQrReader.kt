package cf.construct.mobile.android

import com.google.zxing.BinaryBitmap
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader

internal class PairingQrReader {
    private val reader = QRCodeReader()

    fun read(
        luma: ByteArray,
        width: Int,
        height: Int,
        rowStride: Int,
    ): String? {
        val validDimensions = width > 0 && height > 0 && rowStride >= width
        val requiredBytes = rowStride * (height.toLong() - 1) + width
        if (!validDimensions || requiredBytes > luma.size) return null
        val source = PlanarYUVLuminanceSource(luma, rowStride, height, 0, 0, width, height, false)
        return try {
            reader.decode(BinaryBitmap(HybridBinarizer(source))).text
        } catch (_: ReaderException) {
            null
        }
    }
}
