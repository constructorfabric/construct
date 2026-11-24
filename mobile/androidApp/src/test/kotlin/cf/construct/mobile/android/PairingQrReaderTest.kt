package cf.construct.mobile.android

import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import kotlin.test.Test
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PairingQrReaderTest {
    @Test
    fun readsTheDesktopPayloadFromPaddedCameraRows() {
        val pin = "ab".repeat(32)
        val secret = "cd".repeat(32)
        val payload = "construct-pair://192.0.2.10:47821?v=1&fp=$pin&s=$secret"
        val code = QRCodeWriter().encode(payload, BarcodeFormat.QR_CODE, 280, 280)
        val stride = code.width + 16
        val luma = ByteArray(stride * (code.height - 1) + code.width) { 0xff.toByte() }
        for (y in 0 until code.height) {
            for (x in 0 until code.width) {
                if (code[x, y]) luma[y * stride + x] = 0
            }
        }

        val decoded = PairingQrReader().read(luma, code.width, code.height, stride)
        assertTrue(payload == decoded, "The decoded payload differed.")
    }

    @Test
    fun ignoresBlankAndTruncatedFrames() {
        val reader = PairingQrReader()
        assertNull(reader.read(ByteArray(64), 8, 8, 8))
        assertNull(reader.read(ByteArray(80), 8, 8, 16))
    }
}
