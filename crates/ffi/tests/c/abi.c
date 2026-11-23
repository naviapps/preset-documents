#ifdef __APPLE__
#define _DARWIN_C_SOURCE 1
#endif
#include "preset_documents.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
#ifdef __APPLE__
#include <errno.h>
#include <stdlib.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <unistd.h>
#endif

static void text_result(uint32_t operation, const char *data, const char *options,
                        uint32_t expected_status, char *result, size_t capacity) {
    PresetDocumentsBuffer *owner =
        preset_documents_process(operation, (const uint8_t *)data, data == NULL ? 0 : strlen(data),
                                 (const uint8_t *)options, strlen(options));
    assert(owner != NULL);
    size_t length = 0;
    uint32_t status = 99;
    const uint8_t *bytes = preset_documents_buffer_data(owner, &length, &status);
    assert(bytes != NULL && status == expected_status && length < capacity);
    memcpy(result, bytes, length);
    result[length] = '\0';
    preset_documents_buffer_free(owner);
    assert(strlen(result) == length);
}

static void replacement_request_validation(void) {
    char result[128];
    text_result(8, NULL, "{\"path\":\"file.preset\"}", 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"invalidConfiguration\"}") == 0);
#ifndef __APPLE__
#ifdef _WIN32
    const char *options = "{\"path\":\"C:\\\\preset-documents-abi-probe.preset\"}";
#else
    const char *options = "{\"path\":\"/preset-documents-abi-probe.preset\"}";
#endif
    text_result(8, NULL, options, 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"approvedReplacementUnsupported\"}") == 0);
#endif
}

static void write_budget_validation(void) {
    char result[128];
    text_result(10, NULL, "{\"byteCount\":0,\"maximumByteCount\":3}", 0, result, sizeof result);
    assert(result[0] == '\0');
}

#ifdef __APPLE__
static void write_fixture(const char *path, const char *value) {
    FILE *file = fopen(path, "wb");
    assert(file != NULL);
    size_t length = strlen(value);
    assert(fwrite(value, 1, length, file) == length);
    assert(fclose(file) == 0);
}

static void assert_fixture(const char *path, const char *value) {
    FILE *file = fopen(path, "rb");
    assert(file != NULL);
    char bytes[64];
    size_t length = fread(bytes, 1, sizeof bytes, file);
    assert(!ferror(file) && feof(file));
    assert(length == strlen(value) && memcmp(bytes, value, length) == 0);
    assert(fclose(file) == 0);
}

static void approval_options(char *options, size_t capacity, const char *path,
                             const char *revision) {
    int length =
        snprintf(options, capacity, "{\"path\":\"%s\",\"maximumByteCount\":64,\"revision\":%s}",
                 path, revision);
    assert(length > 0 && (size_t)length < capacity);
}

static void approved_replacement(void) {
    char root[] = "/private/tmp/preset-documents-abi.XXXXXX";
    assert(mkdtemp(root) != NULL);
    char target[128], original[128];
    int length = snprintf(target, sizeof target, "%s/target.preset", root);
    assert(length > 0 && (size_t)length < sizeof target);
    length = snprintf(original, sizeof original, "%s/original.preset", root);
    assert(length > 0 && (size_t)length < sizeof original);
    write_fixture(target, "original");

    char capture[256], revision[4096], replacement[8192], result[4096];
    length = snprintf(capture, sizeof capture, "{\"path\":\"%s\"}", target);
    assert(length > 0 && (size_t)length < sizeof capture);
    struct statfs volume;
    assert(statfs(root, &volume) == 0);
    if (strcmp(volume.f_fstypename, "apfs") != 0 || !(volume.f_flags & MNT_LOCAL)) {
        text_result(8, NULL, capture, 1, result, sizeof result);
        assert(strcmp(result, "{\"code\":\"approvedReplacementUnsupported\"}") == 0);
        assert(unlink(target) == 0);
        assert(rmdir(root) == 0);
        return;
    }
    text_result(8, NULL, capture, 0, revision, sizeof revision);
    assert(revision[0] == '{');
    approval_options(replacement, sizeof replacement, target, revision);

    assert(rename(target, original) == 0);
    text_result(9, "new", replacement, 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"destinationChanged\"}") == 0);
    assert(access(target, F_OK) == -1 && errno == ENOENT);
    write_fixture(target, "another file");
    text_result(9, "new", replacement, 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"destinationChanged\"}") == 0);
    assert_fixture(target, "another file");
    assert_fixture(original, "original");

    text_result(8, NULL, capture, 0, revision, sizeof revision);
    approval_options(replacement, sizeof replacement, target, revision);
    text_result(9, "new", replacement, 0, result, sizeof result);
    assert(strstr(result, "\"state\":\"committed\"") != NULL);
    assert(strstr(result, "\"displacedIdentity\":\"matchesApprovedFile\"") != NULL);
    assert_fixture(target, "new");

    const char key[] = "\"recoveryDirectory\":\"";
    char *directory = strstr(result, key);
    assert(directory != NULL);
    directory += sizeof key - 1;
    char *end = strchr(directory, '"');
    assert(end != NULL);
    *end = '\0';
    /* Inspect and retire only this fixture's one private recovery child. */
    size_t root_length = strlen(root);
    assert(strncmp(directory, root, root_length) == 0 && directory[root_length] == '/');
    const char *name = directory + root_length + 1;
    assert(name[0] != '\0' && strcmp(name, ".") != 0 && strcmp(name, "..") != 0);
    assert(strchr(name, '/') == NULL);
    struct stat metadata;
    assert(lstat(directory, &metadata) == 0 && S_ISDIR(metadata.st_mode));
    assert(metadata.st_uid == getuid() && (metadata.st_mode & 077) == 0);
    char displaced[256];
    length = snprintf(displaced, sizeof displaced, "%s/displaced-preset", directory);
    assert(length > 0 && (size_t)length < sizeof displaced);
    assert_fixture(displaced, "another file");

    assert(unlink(displaced) == 0);
    assert(rmdir(directory) == 0);
    assert(unlink(target) == 0);
    assert(unlink(original) == 0);
    assert(rmdir(root) == 0);
}
#endif

static void metadata_timestamp_projection(void) {
    char result[256];
    text_result(14, "1970-01-01 00:00:00.0000001001+00:30", "", 0, result, sizeof result);
    assert(strcmp(result, "{\"canonical\":\"1970-01-01T00:00:00.0000001+00:30\","
                          "\"unixSeconds\":-1800,\"nanoseconds\":100,\"offsetSeconds\":1800,"
                          "\"hasSubnanosecondPrecision\":true}") == 0);
    text_result(14, "2026-02-29T12:34:56Z", "", 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"invalidDocument\"}") == 0);
    text_result(14, "1970-01-01T00:00:00Z", "{}", 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"invalidConfiguration\"}") == 0);
}

static void document_codec_routes(void) {
    const char *options =
        "{\"format\":{\"identifier\":\"com.example.presets\",\"minimumSupportedVersion\":1,"
        "\"currentVersion\":1},\"limits\":{\"maximumByteCount\":4096,\"maximumDocumentCount\":1,"
        "\"maximumDepth\":128},\"root\":\"collection\"}";
    const char *input = "[ { \"payload\": {\"z\": 1.2300e+04, \"escaped\":\"\\u0061\\/\"},"
                        " \"future\": true, \"formatVersion\": { \"version\": 1,"
                        " \"identifier\": \"com.example.presets\" } } ]";
    const char *normalized = "[{\"formatVersion\":{\"identifier\":\"com.example.presets\","
                             "\"version\":1},\"metadata\":{},\"payload\":{\"z\": 1.2300e+04,"
                             " \"escaped\":\"\\u0061\\/\"}}]";
    const char *compact = "[{\"formatVersion\":{\"identifier\":\"com.example.presets\","
                          "\"version\":1},\"metadata\":{},\"payload\":{\"z\": 1.2300e+04,"
                          " \"escaped\":\"a/\"}}]";
    char result[256];
    text_result(2, input, options, 0, result, sizeof result);
    assert(strcmp(result, normalized) == 0);
    text_result(3, input, options, 0, result, sizeof result);
    assert(strcmp(result, normalized) == 0);
    text_result(11, input, options, 0, result, sizeof result);
    assert(strcmp(result, compact) == 0);

    text_result(2, "[{\"formatVersion\":[\"com.example.presets\",1],\"payload\":42}]", options, 1,
                result, sizeof result);
    assert(strcmp(result, "{\"code\":\"invalidDocument\"}") == 0);
    text_result(3,
                "[{\"formatVersion\":{\"identifier\":\"com.example.presets\",\"version\":1},"
                "\"payload\":9007199254740993},null,{}]",
                options, 1, result, sizeof result);
    assert(
        strcmp(result,
               "{\"code\":\"tooManyDocuments\",\"documentCount\":2,\"maximumDocumentCount\":1}") ==
        0);
    text_result(11,
                "[{\"formatVersion\":{\"identifier\":\"com.example.presets\",\"version\":1},"
                "\"payload\":\"\\ud800\"}]",
                options, 1, result, sizeof result);
    assert(strcmp(result, "{\"code\":\"invalidDocument\"}") == 0);
}

int main(void) {
    metadata_timestamp_projection();
    assert(preset_documents_abi_version() == 1);
    PresetDocumentsTimestampParts timestamp = preset_documents_timestamp_parts(-0.125);
    assert(timestamp.whole_seconds == -1.0 && timestamp.fraction_length == 3);
    assert(memcmp(timestamp.fraction, "875", timestamp.fraction_length) == 0);
    replacement_request_validation();
    write_budget_validation();
#ifdef __APPLE__
    approved_replacement();
#endif
    const char *format = "{\"identifier\":\"com.example.presets\",\"minimumSupportedVersion\":1,"
                         "\"currentVersion\":1}";
    PresetDocumentsBuffer *owner =
        preset_documents_process(1, (const uint8_t *)format, strlen(format), NULL, 0);
    assert(owner != NULL);
    size_t length = 0;
    uint32_t status = 99;
    const uint8_t *bytes = preset_documents_buffer_data(owner, &length, &status);
    assert(bytes != NULL && status == 0 && length > 0);
    unsigned char copy[256];
    assert(length < sizeof copy);
    memcpy(copy, bytes, length);
    preset_documents_buffer_free(owner);
    assert(memcmp(copy, "{", 1) == 0);
    const char *limits =
        "{\"maximumDepth\":128,\"maximumDocumentCount\":1,\"maximumByteCount\":4096}";
    const char *canonical_limits =
        "{\"maximumByteCount\":4096,\"maximumDocumentCount\":1,\"maximumDepth\":128}";
    owner = preset_documents_process(4, (const uint8_t *)limits, strlen(limits), NULL, 0);
    bytes = preset_documents_buffer_data(owner, &length, &status);
    assert(bytes != NULL && status == 0 && length == strlen(canonical_limits));
    assert(memcmp(bytes, canonical_limits, length) == 0);
    preset_documents_buffer_free(owner);
    char invalid_result[64];
    const char *invalid_limits =
        "{\"maximumByteCount\":4096,\"maximumDocumentCount\":1,\"maximumDepth\":0}";
    text_result(4, invalid_limits, "", 1, invalid_result, sizeof invalid_result);
    assert(strcmp(invalid_result, "{\"code\":\"invalidConfiguration\"}") == 0);
    preset_documents_buffer_free(NULL);
    document_codec_routes();
    assert(preset_documents_buffer_data(NULL, &length, &status) == NULL);
    return 0;
}
