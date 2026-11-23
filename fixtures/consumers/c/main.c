#include <CPresetDocuments/preset_documents.h>
#include <assert.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    char bytes[4097];
    size_t length;
    uint32_t status;
} Result;

static Result call(uint32_t operation, const char *data, const char *options) {
    PresetDocumentsBuffer *owner =
        preset_documents_process(operation, (const uint8_t *)data, data == NULL ? 0 : strlen(data),
                                 (const uint8_t *)options, options == NULL ? 0 : strlen(options));
    assert(owner != NULL);
    Result result;
    const uint8_t *bytes = preset_documents_buffer_data(owner, &result.length, &result.status);
    assert(result.length < sizeof result.bytes);
    assert(bytes != NULL || result.length == 0);
    if (result.length != 0) {
        memcpy(result.bytes, bytes, result.length);
    }
    result.bytes[result.length] = '\0';
    preset_documents_buffer_free(owner);
    return result;
}

int main(int argc, char **argv) {
    assert(argc == 2);
    assert(preset_documents_abi_version() == 1);
    Result timestamp = call(14, "1969-12-31t23:59:59.875z", NULL);
    assert(timestamp.status == 0);
    assert(strstr(timestamp.bytes, "\"canonical\":\"1969-12-31T23:59:59.875Z\"") != NULL);
    assert(strstr(timestamp.bytes, "\"unixSeconds\":-1,\"nanoseconds\":875000000") != NULL);
    Result format = call(12,
                         "{\"currentVersion\":1,\"identifier\":\"com.example.external-presets\","
                         "\"minimumSupportedVersion\":1}",
                         NULL);
    assert(format.status == 0);
    assert(strcmp(format.bytes, "{\"identifier\":\"com.example.external-presets\","
                                "\"minimumSupportedVersion\":1,\"currentVersion\":1}") == 0);
    const char *options = "{\"format\":{\"identifier\":\"com.example.external-presets\","
                          "\"minimumSupportedVersion\":1,\"currentVersion\":1},"
                          "\"limits\":{\"maximumByteCount\":4096,\"maximumDocumentCount\":1,"
                          "\"maximumDepth\":128},\"root\":\"document\"}";
    const char *document =
        "{\"formatVersion\":{\"identifier\":\"com.example.external-presets\",\"version\":1},"
        "\"metadata\":{},\"payload\":{\"integer\":18446744073709551615,\"text\":\"日本語😀\"}}";
    Result normalized = call(2, document, options);
    assert(normalized.status == 0);
    Result encoded = call(3, normalized.bytes, options);
    assert(encoded.status == 0);
    assert(strstr(encoded.bytes, "\"integer\":18446744073709551615") != NULL);
    assert(strstr(encoded.bytes, "\"text\":\"日本語😀\"") != NULL);
    Result invalid = call(2, "{", options);
    assert(invalid.status == 1);
    assert(strcmp(invalid.bytes, "{\"code\":\"invalidDocument\"}") == 0);

    /* The producer supplies JSON for one path in this consumer's owned directory. */
    char file_options[8192];
    FILE *file = fopen(argv[1], "rb");
    assert(file != NULL);
    size_t count = fread(file_options, 1, sizeof file_options - 1, file);
    assert(!ferror(file) && feof(file));
    assert(fclose(file) == 0);
    file_options[count] = '\0';
    Result created = call(6, encoded.bytes, file_options);
    assert(created.status == 0 && created.length == 0);
    Result collision = call(7, "must not replace", file_options);
    assert(collision.status == 1);
    assert(strcmp(collision.bytes, "{\"code\":\"destinationAlreadyExists\"}") == 0);
    Result saved = call(5, NULL, file_options);
    assert(saved.status == 0 && saved.length == encoded.length);
    assert(memcmp(saved.bytes, encoded.bytes, encoded.length) == 0);
    puts("Standalone installed C consumer passed UInt64/Unicode, typed rejection and no-clobber.");
    return 0;
}
