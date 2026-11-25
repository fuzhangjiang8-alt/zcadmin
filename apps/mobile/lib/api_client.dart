// =============================================================================
// API 客户端：保存会话 Cookie、统一错误处理。
// =============================================================================

import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:shared_preferences/shared_preferences.dart';

/// 后端地址：真机调试时改成局域网 IP。
const String apiBase = String.fromEnvironment(
  'API_BASE',
  defaultValue: 'http://10.0.2.2:8080',
);

/// 后端返回的业务错误。
class ApiException implements Exception {
  ApiException(this.message);
  final String message;

  @override
  String toString() => message;
}

/// 极简 API 客户端：保存会话 Cookie，统一错误处理。
class ApiClient {
  String? _cookie;

  /// 从本地恢复会话（冷启动时仍可请求）。
  Future<void> restore() async {
    final prefs = await SharedPreferences.getInstance();
    final saved = prefs.getString('zcadmin.cookie');
    if (saved != null && saved.isNotEmpty) {
      _cookie = saved;
    }
  }

  Map<String, String> get _headers => {
        'content-type': 'application/json',
        if (_cookie != null) 'cookie': _cookie!,
      };

  Future<void> login(String username, String password, String? tenant) async {
    final resp = await http.post(
      Uri.parse('$apiBase/api/auth/login'),
      headers: {'content-type': 'application/json'},
      body: jsonEncode({
        'username': username,
        'password': password,
        'tenant': (tenant == null || tenant.isEmpty) ? null : tenant,
      }),
    );

    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }

    final setCookie = resp.headers['set-cookie'];
    if (setCookie == null) {
      throw ApiException('服务端未返回会话 Cookie');
    }
    _cookie = setCookie.split(';').first;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('zcadmin.cookie', _cookie!);
  }

  Future<List<Map<String, dynamic>>> customers() async {
    final resp = await http.get(
      Uri.parse('$apiBase/api/crm/customers?page=1&size=50'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    final data = (body['data'] as Map<String, dynamic>)['data'] as List<dynamic>;
    return data.cast<Map<String, dynamic>>();
  }

  Future<Map<String, dynamic>> _getObject(String path) async {
    final resp = await http.get(Uri.parse('$apiBase$path'), headers: _headers);
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    return body['data'] as Map<String, dynamic>;
  }

  Future<List<Map<String, dynamic>>> _getList(String path) async {
    final resp = await http.get(Uri.parse('$apiBase$path'), headers: _headers);
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    return (body['data'] as List<dynamic>).cast<Map<String, dynamic>>();
  }

  Future<Map<String, dynamic>> customer(String id) =>
      _getObject('/api/crm/customers/$id');

  Future<List<Map<String, dynamic>>> contacts(String id) =>
      _getList('/api/crm/customers/$id/contacts');

  Future<List<Map<String, dynamic>>> opportunities(String id) =>
      _getList('/api/crm/customers/$id/opportunities');

  Future<List<Map<String, dynamic>>> contracts(String id) =>
      _getList('/api/crm/customers/$id/contracts');

  Future<List<Map<String, dynamic>>> opportunityList() async {
    final resp = await http.get(
      Uri.parse('$apiBase/api/crm/opportunities?page=1&size=50'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    final page = body['data'] as Map<String, dynamic>;
    return (page['data'] as List<dynamic>).cast<Map<String, dynamic>>();
  }

  Future<Map<String, dynamic>> opportunity(String id) =>
      _getObject('/api/crm/opportunities/$id');

  Future<List<Map<String, dynamic>>> opportunityContracts(String id) =>
      _getList('/api/crm/opportunities/$id/contracts');

  Future<List<Map<String, dynamic>>> opportunityStageHistory(String id) =>
      _getList('/api/crm/opportunities/$id/stage-history');

  Future<Map<String, dynamic>> dashboardSummary() =>
      _getObject('/api/crm/dashboard/summary');

  Future<List<Map<String, dynamic>>> _alertPage(String path) async {
    final resp = await http.get(Uri.parse('$apiBase$path'), headers: _headers);
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    final page = body['data'] as Map<String, dynamic>;
    return (page['data'] as List<dynamic>).cast<Map<String, dynamic>>();
  }

  Future<List<Map<String, dynamic>>> auditAlerts() =>
      _alertPage('/api/system/audit-alerts?page=1&size=50&unacked=true');

  Future<List<Map<String, dynamic>>> subscribedAlerts() =>
      _alertPage('/api/system/audit-alerts/subscribed?page=1&size=50');

  Future<List<String>> subscriptions() async {
    final resp = await http.get(
      Uri.parse('$apiBase/api/system/audit-alert-subscriptions'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    return (body['data'] as List<dynamic>)
        .map((e) => (e as Map<String, dynamic>)['actionPattern'].toString())
        .toList();
  }

  Future<void> saveSubscriptions(List<String> patterns) async {
    final resp = await http.put(
      Uri.parse('$apiBase/api/system/audit-alert-subscriptions'),
      headers: {..._headers, 'content-type': 'application/json'},
      body: jsonEncode({'patterns': patterns}),
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
  }

  Future<List<Map<String, dynamic>>> alertRules() async {
    final resp = await http.get(
      Uri.parse('$apiBase/api/system/audit-alert-rules'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    return (body['data'] as List<dynamic>).cast<Map<String, dynamic>>();
  }

  Future<void> saveAlertRules(List<Map<String, dynamic>> rules) async {
    final resp = await http.put(
      Uri.parse('$apiBase/api/system/audit-alert-rules'),
      headers: {..._headers, 'content-type': 'application/json'},
      body: jsonEncode({'rules': rules}),
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
  }

  Future<void> ackAuditAlert(String id) async {
    final resp = await http.post(
      Uri.parse('$apiBase/api/system/audit-alerts/$id/ack'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
  }

  Future<List<Map<String, dynamic>>> contractList() async {
    final resp = await http.get(
      Uri.parse('$apiBase/api/crm/contracts?page=1&size=50'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    final page = body['data'] as Map<String, dynamic>;
    return (page['data'] as List<dynamic>).cast<Map<String, dynamic>>();
  }

  Future<Map<String, dynamic>> contract(String id) =>
      _getObject('/api/crm/contracts/$id');

  Future<Map<String, dynamic>> contractPayments(String id) =>
      _getObject('/api/crm/contracts/$id/payments?page=1&size=50');

  Future<List<Map<String, dynamic>>> paymentList({
    String? keyword,
    DateTime? from,
    DateTime? to,
  }) async {
    String date(DateTime v) =>
        '${v.year.toString().padLeft(4, '0')}-${v.month.toString().padLeft(2, '0')}-${v.day.toString().padLeft(2, '0')}';
    final params = <String>['page=1', 'size=50'];
    if (keyword != null && keyword.isNotEmpty) {
      params.add('keyword=${Uri.encodeQueryComponent(keyword)}');
    }
    if (from != null) params.add('paidFrom=${date(from)}');
    if (to != null) params.add('paidTo=${date(to)}');
    final resp = await http.get(
      Uri.parse('$apiBase/api/crm/payments?${params.join('&')}'),
      headers: _headers,
    );
    if (resp.statusCode != 200) {
      throw ApiException(_problemDetail(resp));
    }
    final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
    final page = body['data'] as Map<String, dynamic>;
    return (page['data'] as List<dynamic>).cast<Map<String, dynamic>>();
  }

  /// 解析 RFC 9457 problem+json 的 detail。
  String _problemDetail(http.Response resp) {
    try {
      final body = jsonDecode(utf8.decode(resp.bodyBytes)) as Map<String, dynamic>;
      return (body['detail'] ?? body['title'] ?? '请求失败').toString();
    } catch (_) {
      return '请求失败（HTTP ${resp.statusCode}）';
    }
  }
}
