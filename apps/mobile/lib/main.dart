// =============================================================================
// zcadmin 移动端入口。
//
// 页面：
//   lib/pages/login_page.dart
//   lib/pages/home_page.dart
//   lib/pages/customers_page.dart
//   lib/pages/customer_detail_page.dart
//   lib/pages/opportunities_page.dart
//   lib/pages/opportunity_detail_page.dart
// =============================================================================

import 'package:flutter/material.dart';

import 'pages/login_page.dart';

void main() => runApp(const ZcAdminApp());

class ZcAdminApp extends StatelessWidget {
  const ZcAdminApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'zcadmin',
      theme: ThemeData(colorSchemeSeed: const Color(0xFF1677FF), useMaterial3: true),
      home: const LoginPage(),
    );
  }
}
