// =============================================================================
// 合同详情：回款进度 + 最近回款记录。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';

class ContractDetailPage extends StatefulWidget {
  const ContractDetailPage({
    super.key,
    required this.api,
    required this.contractId,
  });

  final ApiClient api;
  final String contractId;

  @override
  State<ContractDetailPage> createState() => _ContractDetailPageState();
}

class _ContractDetailPageState extends State<ContractDetailPage> {
  late final Future<List<dynamic>> _future = Future.wait([
    widget.api.contract(widget.contractId),
    widget.api.contractPayments(widget.contractId),
  ]);

  String _money(dynamic cents) {
    final value = (cents as num? ?? 0) / 100;
    return '¥${value.toStringAsFixed(2)}';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('合同详情')),
      body: FutureBuilder<List<dynamic>>(
        future: _future,
        builder: (context, snapshot) {
          if (snapshot.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snapshot.hasError) {
            return Center(child: Text(snapshot.error.toString()));
          }
          final data = snapshot.data!;
          final contract = data[0] as Map<String, dynamic>;
          final summary = data[1] as Map<String, dynamic>;
          final items = (summary['items'] as List).cast<Map<String, dynamic>>();

          final amount = (summary['contractAmountCents'] as num? ?? 0).toDouble();
          final paid = (summary['paidCents'] as num? ?? 0).toDouble();
          final progress = amount <= 0 ? 0.0 : (paid / amount).clamp(0.0, 1.0).toDouble();

          return ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${contract['contractNo']}',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      const SizedBox(height: 4),
                      Text('${contract['name']}'),
                      const SizedBox(height: 4),
                      Text('状态：${contract['status']}'),
                      const SizedBox(height: 12),
                      LinearProgressIndicator(value: progress),
                      const SizedBox(height: 8),
                      Text('合同金额：${_money(summary['contractAmountCents'])}'),
                      Text('已回款：${_money(summary['paidCents'])}'),
                      Text('未回款：${_money(summary['remainingCents'])}'),
                    ],
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.only(top: 20, bottom: 8),
                child: Text(
                  '回款记录（最近 ${items.length} 笔）',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ),
              if (items.isEmpty)
                const Text('暂无回款记录', style: TextStyle(color: Colors.grey))
              else
                ...items.map(
                  (p) => ListTile(
                    dense: true,
                    title: Text('${p['paymentNo']}'),
                    subtitle: Text('${p['paidAt'] ?? '-'}'),
                    trailing: Text(_money(p['amountCents'])),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }
}
