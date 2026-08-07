package dev.voiceos.client

import android.content.res.AssetManager
import com.k2fsa.sherpa.onnx.FeatureConfig
import com.k2fsa.sherpa.onnx.KeywordSpotter
import com.k2fsa.sherpa.onnx.KeywordSpotterConfig
import com.k2fsa.sherpa.onnx.OnlineModelConfig
import com.k2fsa.sherpa.onnx.OnlineStream
import com.k2fsa.sherpa.onnx.OnlineTransducerModelConfig

/** Thin lifecycle-safe wrapper around sherpa-onnx open-vocabulary keyword spotting. */
class SherpaWakeWordEngine(assetManager: AssetManager) : AutoCloseable {
    private val modelDirectory = "sherpa-kws-gigaspeech-3.3m"
    private val spotter = KeywordSpotter(
        assetManager = assetManager,
        config = KeywordSpotterConfig(
            featConfig = FeatureConfig(sampleRate = SAMPLE_RATE, featureDim = 80),
            modelConfig = OnlineModelConfig(
                transducer = OnlineTransducerModelConfig(
                    encoder = "$modelDirectory/encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
                    decoder = "$modelDirectory/decoder-epoch-12-avg-2-chunk-16-left-64.onnx",
                    joiner = "$modelDirectory/joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
                ),
                tokens = "$modelDirectory/tokens.txt",
                numThreads = 2,
                modelType = "zipformer2",
            ),
            keywordsFile = "$modelDirectory/keywords.txt",
            keywordsScore = WakeWordSettings.KEYWORD_SCORE,
            keywordsThreshold = WakeWordSettings.KEYWORD_THRESHOLD,
            numTrailingBlanks = 2,
        ),
    )
    private var stream: OnlineStream = spotter.createStream()

    fun accept(samples: FloatArray): String? {
        stream.acceptWaveform(samples, SAMPLE_RATE)
        while (spotter.isReady(stream)) {
            spotter.decode(stream)
            val keyword = spotter.getResult(stream).keyword
            if (keyword.isNotBlank()) {
                spotter.reset(stream)
                return keyword
            }
        }
        return null
    }

    fun reset() = spotter.reset(stream)

    override fun close() {
        stream.release()
        spotter.release()
    }

    companion object {
        const val SAMPLE_RATE = 16_000
    }
}
