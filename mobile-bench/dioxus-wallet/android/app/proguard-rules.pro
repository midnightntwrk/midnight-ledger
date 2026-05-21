# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class {{package}}.* {
  native <methods>;
}

-keep class {{package}}.WryActivity {
  public <init>(...);

  void setWebView({{package}}.RustWebView);
  java.lang.Class getAppClass(...);
  java.lang.String getVersion();
}

-keep class {{package}}.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class {{package}}.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void setAutoPlay(...);
  void setUserAgent(...);
  void evalScript(...);
}

-keep class {{package}}.RustWebChromeClient,{{package}}.RustWebViewClient {
  public <init>(...);
}