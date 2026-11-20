// Exercise the production Activity/JNI startup path without a compiler/runtime build.
#include <jni.h>
JNIEXPORT void JNICALL Java_com_perry_app_PerryBridge_nativeInit(JNIEnv *env, jclass cls) {}
JNIEXPORT void JNICALL Java_com_perry_app_PerryBridge_nativeShutdown(JNIEnv *env, jclass cls) {}
JNIEXPORT void JNICALL Java_com_perry_app_PerryBridge_nativeMemoryPressure(JNIEnv *env, jclass cls, jint level) {}
JNIEXPORT void JNICALL Java_com_perry_app_PerryBridge_nativeMain(JNIEnv *env, jclass cls) {
    jclass probe = (*env)->FindClass(env, "com/perry/app/NightModeTest");
    if (!probe) return;
    jmethodID entry = (*env)->GetStaticMethodID(env, probe, "onNativeMain", "()V");
    if (entry) (*env)->CallStaticVoidMethod(env, probe, entry);
}
