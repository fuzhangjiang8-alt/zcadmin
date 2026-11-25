// =============================================================================
// 经营看板：合同额 / 回款 / 客户 / 商机 + 近 6 个月趋势
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'alerts_page.dart';

class DashboardPage extends StatefulWidget {
  const DashboardPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<DashboardPage> createState() => _DashboardPageState();
}

class _DashboardPageState extends State<DashboardPage> {
  late Future<Map<String, dynamic>> _future = widget.api.dashboardSummary();

  Future<void> _refresh() async {
    setState(() => _future = widget.api.dashboardSummary());
    await _future;
  }

  String _money(dynamic cents) {
    final value = (cents as num? ?? 0) / 100;
    if (value >= 10000) {
      return '¥${(value / 10000).toStringAsFixed(1)} 万';
    }
    return '¥${value.toStringAsFixed(2)}';
  }

  @override
  Widget build(BuildContext context) {
    return RefreshIndicator(
      onRefresh: _refresh,
      child: FutureBuilder<Map<String, dynamic>>(
        future: _future,
        builder: (context, snapshot) {
          if (snapshot.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snapshot.hasError) {
            return Center(child: Text(snapshot.error.toString()));
          }
          final data = snapshot.data!;
          final months = (data['months'] as List? ?? []).cast<Map<String, dynamic>>();

          return ListView(
            padding: const EdgeInsets.all(16),
            children: [
              GridView.count(
                crossAxisCount: 2,
                shrinkWrap: true,
                physics: const NeverScrollableScrollPhysics(),
                mainAxisSpacing: 12,
                crossAxisSpacing: 12,
                childAspectRatio: 1.7,
                children: [
                  _stat('合同金额', _money(data['contractAmountCents']), Colors.blue),
                  _stat('已回款', _money(data['paidCents']), Colors.green),
                  _stat('未回款', _money(data['remainingCents']), Colors.orange),
                  _stat('合同数', '${data['contractCount'] ?? 0}', Colors.purple),
                  _stat('客户数', '${data['customerCount'] ?? 0}', Colors.teal),
                  _stat('商机数', '${data['opportunityCount'] ?? 0}', Colors.indigo),
                ],
              ),
              Card(
                margin: const EdgeInsets.only(top: 16),
                child: ListTile(
                  leading: const Icon(Icons.warning_amber, color: Colors.orange),
                  title: const Text('审计告警'),
                  subtitle: const Text('查看未处理告警'),
                  trailing: const Icon(Icons.chevron_right),
                  onTap: () {
                    Navigator.of(context).push(
                      MaterialPageRoute(builder: (_) => AlertsPage(api: widget.api)),
                    );
                  },
                ),
              ),
              const Padding(
                padding: EdgeInsets.only(top: 24, bottom: 8),
                child: Text('近 6 个月', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
              ),
              if (months.isEmpty)
                const Text('暂无趋势数据', style: TextStyle(color: Colors.grey))
              else
                ...months.map(
                  (m) => ListTile(
                    dense: true,
                    title: Text('${m['month']}'),
                    subtitle: Text('合同 ${_money(m["contractCents"])}'),
                    trailing: Text(
                      '回款 ${_money(m["paymentCents"])}',
                      style: const TextStyle(fontSize: 12),
                    ),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }

  Widget _stat(String label, String value, Color color) {
    return Card(
      margin: EdgeInsets.zero,
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            Text(label, style: TextStyle(color: color, fontSize: 13)),
            const SizedBox(height: 6),
            Text(value, style: const TextStyle(fontSize: 18, fontWeight: FontWeight.w600)),
          ],
        ),
      ),
    );
  }
}
