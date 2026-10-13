package io.github.fushugaku.rustias;

import android.annotation.SuppressLint;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Insets;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import android.widget.Toast;

import androidx.activity.ComponentActivity;
import androidx.activity.OnBackPressedCallback;

import androidx.webkit.JavaScriptReplyProxy;
import androidx.webkit.WebViewAssetLoader;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;

import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.util.Arrays;
import java.util.Collections;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MainActivity extends ComponentActivity {
    private static final String ORIGIN = "https://appassets.androidplatform.net";
    private static final String ENTRY = ORIGIN + "/assets/public/index.html?interface=new";
    private static final int PICK_FILE = 1, SAVE_FILE = 2;
    private final ExecutorService files = Executors.newSingleThreadExecutor();
    private WebView webView;
    private FrameLayout root;
    private ValueCallback<Uri[]> filePicker;
    private ExportTransfer transfer;
    private File exportFile;
    private JavaScriptReplyProxy saveReply;
    private int saveRequest;
    private boolean fullscreen;
    private boolean destroyed;
    private boolean rendererGone;
    private View customView;
    private WebChromeClient.CustomViewCallback customViewCallback;

    @SuppressLint("SetJavaScriptEnabled")
    @Override public void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        root = new FrameLayout(this);
        root.setBackgroundColor(Color.rgb(203, 208, 213));
        webView = new WebView(this);
        root.addView(webView, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
        setContentView(root);
        configureInsets();
        setVolumeControlStream(android.media.AudioManager.STREAM_MUSIC);
        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG);
        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(true); // User-selected content:// uploads from the document picker.
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setMediaPlaybackRequiresUserGesture(true);
        settings.setSupportMultipleWindows(false);

        WebViewAssetLoader loader = new WebViewAssetLoader.Builder()
            .addPathHandler("/assets/", new WebViewAssetLoader.AssetsPathHandler(this)).build();
        webView.setWebViewClient(new WebViewClient() {
            @Override public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
                rendererGone = true;
                showMessage("The audio interface stopped. Reopen Rustias to restore your saved session.");
                finish();
                return true;
            }
            @Override public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                Uri url = request.getUrl();
                if (isLocal(url)) {
                    WebResourceResponse response = loader.shouldInterceptRequest(url);
                    if (response != null) {
                        response.setResponseHeaders(Collections.singletonMap("Cache-Control", "no-store"));
                        // JS modules and streaming WASM require the correct MIME types.
                        String path = url.getPath();
                        if (path != null && path.endsWith(".wasm")) response.setMimeType("application/wasm");
                        else if (path != null && path.endsWith(".js")) response.setMimeType("text/javascript");
                        return response;
                    }
                }
                if ("blob".equals(url.getScheme()) || "data".equals(url.getScheme())) return null;
                return new WebResourceResponse("text/plain", "UTF-8", 404, "Not Found", Collections.emptyMap(), new ByteArrayInputStream(new byte[0]));
            }
            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                Uri url = request.getUrl();
                if (isLocal(url)) return false;
                if (request.isForMainFrame() && ("https".equals(url.getScheme()) || "http".equals(url.getScheme()))) {
                    try { startActivity(new Intent(Intent.ACTION_VIEW, url)); }
                    catch (ActivityNotFoundException error) { showMessage("No browser is available to open this link."); }
                }
                return true;
            }
        });
        webView.setWebChromeClient(new WebChromeClient() {
            @Override public boolean onShowFileChooser(WebView view, ValueCallback<Uri[]> callback, FileChooserParams params) {
                if (filePicker != null) filePicker.onReceiveValue(null);
                filePicker = callback;
                Intent intent = new Intent(Intent.ACTION_GET_CONTENT).addCategory(Intent.CATEGORY_OPENABLE);
                String[] accepts = params.getAcceptTypes();
                boolean extensions = Arrays.stream(accepts).anyMatch(value -> value.contains("."));
                String[] types = Arrays.stream(accepts).filter(value -> value.contains("/")).toArray(String[]::new);
                intent.setType(!extensions && types.length == 1 ? types[0] : "*/*");
                if (!extensions && types.length > 1) intent.putExtra(Intent.EXTRA_MIME_TYPES, types);
                intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, params.getMode() == FileChooserParams.MODE_OPEN_MULTIPLE);
                try { startActivityForResult(intent, PICK_FILE); }
                catch (ActivityNotFoundException error) { filePicker.onReceiveValue(null); filePicker = null; showMessage("No file picker is available."); }
                return true;
            }
            @Override public void onShowCustomView(View view, CustomViewCallback callback) {
                if (customView != null) { callback.onCustomViewHidden(); return; }
                customView = view;
                customViewCallback = callback;
                root.addView(view, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
                webView.setVisibility(View.GONE);
                setFullscreen(true);
            }
            @Override public void onHideCustomView() { hideCustomView(); }
        });
        if (WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            WebViewCompat.addWebMessageListener(webView, "RustiasFiles", Collections.singleton(ORIGIN),
                (view, message, sourceOrigin, isMainFrame, reply) -> {
                    if (!isMainFrame || !isLocal(sourceOrigin) || destroyed) return;
                    String data = message.getData();
                    if (data != null && data.length() <= 180_000) handleMessage(data, reply);
                });
        } else showMessage("Update Android System WebView to enable file exports.");
        getOnBackPressedDispatcher().addCallback(this, new OnBackPressedCallback(true) {
            @Override public void handleOnBackPressed() { handleBack(); }
        });
        if (savedInstanceState == null || webView.restoreState(savedInstanceState) == null) webView.loadUrl(ENTRY);
    }

    private static boolean isLocal(Uri url) {
        return "https".equals(url.getScheme()) && "appassets.androidplatform.net".equals(url.getHost()) && (url.getPort() == -1 || url.getPort() == 443);
    }

    private void handleMessage(String data, JavaScriptReplyProxy reply) {
        final JSONObject message;
        final int request;
        try { message = new JSONObject(data); request = message.getInt("request"); }
        catch (JSONException error) { return; }
        if ("fullscreen".equals(message.optString("type"))) {
            setFullscreen(message.optBoolean("active"));
            reply(reply, request, true, "fullscreen", fullscreen);
            return;
        }
        files.execute(() -> {
            try {
                switch (message.getString("type")) {
                    case "begin":
                        if (transfer != null) throw new IOException("Finish the current file export first.");
                        transfer = new ExportTransfer(getCacheDir(), message.getString("name"), message.getString("mime"), message.getLong("size"));
                        reply(reply, request, true, "offset", 0);
                        break;
                    case "chunk":
                        if (transfer == null || exportFile != null) throw new IOException("No file export is active.");
                        long offset = transfer.append(message.getLong("offset"), message.getString("data"));
                        reply(reply, request, true, "offset", offset);
                        break;
                    case "finish":
                        if (transfer == null || exportFile != null) throw new IOException("No file export is active.");
                        exportFile = transfer.finish();
                        String name = transfer.name, mime = transfer.mime;
                        runOnUiThread(() -> {
                            if (destroyed) return;
                            saveReply = reply;
                            saveRequest = request;
                            Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(mime).putExtra(Intent.EXTRA_TITLE, name);
                            try { startActivityForResult(intent, SAVE_FILE); }
                            catch (ActivityNotFoundException error) { completeExport(null); }
                        });
                        break;
                    case "abort":
                        clearTransfer();
                        reply(reply, request, true, "cancelled", true);
                        break;
                    default: throw new IOException("Unknown file export command.");
                }
            } catch (Exception error) {
                try { clearTransfer(); } catch (IOException ignored) { }
                reply(reply, request, false, "error", error.getMessage());
            }
        });
    }

    @Override protected void onActivityResult(int requestCode, int resultCode, Intent intent) {
        super.onActivityResult(requestCode, resultCode, intent);
        if (requestCode == PICK_FILE && filePicker != null) {
            filePicker.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(resultCode, intent));
            filePicker = null;
        } else if (requestCode == SAVE_FILE) {
            completeExport(resultCode == RESULT_OK && intent != null ? intent.getData() : null);
        }
    }

    private void completeExport(Uri destination) {
        JavaScriptReplyProxy callback = saveReply;
        int request = saveRequest;
        saveReply = null;
        files.execute(() -> {
            try {
                if (destination != null && exportFile != null) {
                    try (FileInputStream source = new FileInputStream(exportFile); OutputStream output = getContentResolver().openOutputStream(destination, "wt")) {
                        if (output == null) throw new IOException("Could not open the destination file.");
                        byte[] buffer = new byte[64 * 1024];
                        int count;
                        while ((count = source.read(buffer)) != -1) output.write(buffer, 0, count);
                    }
                    runOnUiThread(() -> showMessage("File saved."));
                }
                reply(callback, request, true, "cancelled", destination == null);
            } catch (IOException error) {
                reply(callback, request, false, "error", "Could not save the file: " + error.getMessage());
            } finally {
                try { clearTransfer(); } catch (IOException ignored) { }
            }
        });
    }

    private void clearTransfer() throws IOException {
        try { if (transfer != null) transfer.close(); }
        finally { transfer = null; exportFile = null; }
    }

    private void reply(JavaScriptReplyProxy callback, int request, boolean ok, String key, Object value) {
        if (callback == null) return;
        try {
            String result = new JSONObject().put("request", request).put("ok", ok).put(key, value).toString();
            runOnUiThread(() -> { if (!destroyed && WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) { try { callback.postMessage(result); } catch (IllegalStateException ignored) { } } });
        } catch (JSONException ignored) { }
    }

    private void configureInsets() {
        if (Build.VERSION.SDK_INT >= 30) getWindow().setDecorFitsSystemWindows(false);
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            if (Build.VERSION.SDK_INT >= 30) {
                Insets edges = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.ime() | WindowInsets.Type.displayCutout());
                view.setPadding(edges.left, edges.top, edges.right, edges.bottom);
            } else view.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(), insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            return insets;
        });
        setFullscreen(false);
    }

    private void setFullscreen(boolean active) {
        fullscreen = active;
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController bars = getWindow().getInsetsController();
            if (bars != null) {
                bars.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                if (active) bars.hide(WindowInsets.Type.systemBars());
                else bars.show(WindowInsets.Type.systemBars());
            }
        } else {
            int flags = View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
            if (active) flags |= View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY;
            getWindow().getDecorView().setSystemUiVisibility(flags);
        }
        root.requestApplyInsets();
    }

    private void hideCustomView() {
        if (customView == null) return;
        root.removeView(customView);
        customView = null;
        webView.setVisibility(View.VISIBLE);
        setFullscreen(false);
        customViewCallback.onCustomViewHidden();
        customViewCallback = null;
    }

    private void handleBack() {
        if (customView != null) { hideCustomView(); return; }
        if (fullscreen) {
            setFullscreen(false);
            webView.evaluateJavascript("window.RustiasAndroid?.setFullscreen(false)", null);
            return;
        }
        webView.evaluateJavascript("window.RustiasAndroid?.back() === true", handled -> {
            if ("true".equals(handled)) return;
            if (webView.canGoBack()) webView.goBack();
            else finish();
        });
    }

    @Override protected void onPause() {
        if (!rendererGone) {
            webView.evaluateJavascript("window.RustiasAndroid?.pause()", null);
            webView.onPause();
        }
        super.onPause();
    }
    @Override protected void onResume() { super.onResume(); if (webView != null && !rendererGone) webView.onResume(); }
    @Override protected void onSaveInstanceState(Bundle state) { if (!rendererGone) webView.saveState(state); super.onSaveInstanceState(state); }
    @Override protected void onDestroy() {
        destroyed = true;
        if (filePicker != null) filePicker.onReceiveValue(null);
        files.execute(() -> { try { clearTransfer(); } catch (IOException ignored) { } });
        files.shutdown();
        root.removeAllViews();
        webView.destroy();
        super.onDestroy();
    }
    private void showMessage(String message) { if (!destroyed) Toast.makeText(this, message, Toast.LENGTH_SHORT).show(); }
}
