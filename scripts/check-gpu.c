/* Headless Pi GPU check: create a real GLES2 context and require the V3D driver.
 * Cross-build with the Buildroot compiler; no HDMI output is required. */
#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    PFNEGLGETPLATFORMDISPLAYEXTPROC get_display =
        (PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
    if (!get_display) { fprintf(stderr, "EGL platform display unavailable\n"); return 1; }
    EGLDisplay display = get_display(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL);
    EGLint major, minor;
    if (display == EGL_NO_DISPLAY || !eglInitialize(display, &major, &minor)) {
        fprintf(stderr, "EGL initialization failed: 0x%x\n", eglGetError()); return 1;
    }
    EGLint attributes[] = {EGL_SURFACE_TYPE, EGL_PBUFFER_BIT,
        EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_RED_SIZE, 8,
        EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_NONE};
    EGLConfig config;
    EGLint count;
    if (!eglBindAPI(EGL_OPENGL_ES_API) ||
        !eglChooseConfig(display, attributes, &config, 1, &count) || count < 1) {
        fprintf(stderr, "GLES2 EGL config unavailable: 0x%x\n", eglGetError()); return 1;
    }
    EGLint context_attributes[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    EGLint surface_attributes[] = {EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE};
    EGLContext context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attributes);
    EGLSurface surface = eglCreatePbufferSurface(display, config, surface_attributes);
    if (context == EGL_NO_CONTEXT || surface == EGL_NO_SURFACE ||
        !eglMakeCurrent(display, surface, surface, context)) {
        fprintf(stderr, "GLES2 context failed: 0x%x\n", eglGetError()); return 1;
    }
    const char *renderer = (const char *)glGetString(GL_RENDERER);
    printf("EGL %d.%d\nGL vendor: %s\nGL renderer: %s\nGL version: %s\n",
        major, minor, glGetString(GL_VENDOR), renderer, glGetString(GL_VERSION));
    int ok = renderer && strstr(renderer, "V3D");
    eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroySurface(display, surface);
    eglDestroyContext(display, context);
    eglTerminate(display);
    if (!ok) fprintf(stderr, "Expected Pi V3D GPU renderer\n");
    return ok ? 0 : 1;
}
