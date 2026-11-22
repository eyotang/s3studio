#include <Security/Security.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char service[] = "com.s3studio.desktop.signing-test";
static const char account[] = "fixture";
static const char secret[] = "synthetic test value";

static void check(OSStatus status, const char *operation) {
    if (status != errSecSuccess) {
        fprintf(stderr, "%s failed with Keychain status %d\n", operation, (int)status);
        exit(1);
    }
}

int main(int argc, char **argv) {
    if (argc != 3) {
        return 2;
    }

    SecKeychainRef keychain = NULL;
    const char *path = argv[1];
    const char *password = argv[2];
    UInt32 password_length = (UInt32)strlen(password);
    check(SecKeychainSetUserInteractionAllowed(false), "disable prompts");
#if PROBE_BUILD == 1
    check(SecKeychainCreate(path, password_length, password, false, NULL, &keychain),
          "create fixture keychain");
    check(SecKeychainAddGenericPassword(keychain, sizeof(service) - 1, service,
                                       sizeof(account) - 1, account, sizeof(secret) - 1,
                                       secret, NULL),
          "add credential from build 1");
#else
    check(SecKeychainOpen(path, &keychain), "open fixture keychain");
    check(SecKeychainUnlock(keychain, password_length, password, true), "unlock fixture keychain");
    UInt32 length = 0;
    void *value = NULL;
    check(SecKeychainFindGenericPassword(keychain, sizeof(service) - 1, service,
                                        sizeof(account) - 1, account, &length, &value, NULL),
          "read build 1 credential from build 2");
    bool matches = length == sizeof(secret) - 1 && memcmp(value, secret, length) == 0;
    SecKeychainItemFreeContent(NULL, value);
    if (!matches) {
        return 1;
    }
#endif
    CFRelease(keychain);
    return 0;
}
