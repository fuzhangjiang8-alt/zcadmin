// =============================================================================
// 客户详情：基本信息 + 联系人 + 商机 + 合同。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';

class CustomerDetailPage extends StatefulWidget {
  const CustomerDetailPage({
    super.key,
    required this.api,
    required this.customerId,
  });

  final ApiClient api;
  final String customerId;

  @override
  State<CustomerDetailPage> createState() => _CustomerDetailPageState();
}

class _CustomerDetailPageState extends State<CustomerDetailPage> {
  late final Future<List<dynamic>> _future = Future.wait([
    widget.api.customer(widget.customerId),
    widget.api.contacts(widget.customerId),
    widget.api.opportunities(widget.customerId),
    widget.api.contracts(widget.customerId),
  ]);

  String _money(dynamic cents) {
    final value = (cents as num? ?? 0) / 100;
    return '¥${value.toStringAsFixed(2)}';
  }

  Widget _section(String title, List<Widget> children) {
    return Padding(
      padding: const EdgeInsets.only(top: 20),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(title, style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          if (children.isEmpty)
            const Text('暂无数据', style: TextStyle(color: Colors.grey))
          else
            ...children,
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('客户详情')),
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
          final customer = data[0] as Map<String, dynamic>;
          final contacts = (data[1] as List).cast<Map<String, dynamic>>();
          final opportunities = (data[2] as List).cast<Map<String, dynamic>>();
          final contracts = (data[3] as List).cast<Map<String, dynamic>>();

          return ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Card(
                child: ListTile(
                  title: Text('${customer['name']}'),
                  subtitle: Text('${customer['code']} · ${customer['level']}'),
                  trailing: Text('${customer['status']}'),
                ),
              ),
              _section(
                '联系人（${contacts.length}）',
                contacts
                    .map(
                      (c) => ListTile(
                        dense: true,
                        title: Text('${c['name']}'),
                        subtitle: Text('${c['phone'] ?? '-'}'),
                        trailing: c['isPrimary'] == true
                            ? const Icon(Icons.star, size: 18, color: Colors.orange)
                            : null,
                      ),
                    )
                    .toList(),
              ),
              _section(
                '商机（${opportunities.length}）',
                opportunities
                    .map(
                      (o) => ListTile(
                        dense: true,
                        title: Text('${o['name']}'),
                        subtitle: Text('${o['stage']}'),
                        trailing: Text(_money(o['amountCents'])),
                      ),
                    )
                    .toList(),
              ),
              _section(
                '合同（${contracts.length}）',
                contracts
                    .map(
                      (c) => ListTile(
                        dense: true,
                        title: Text('${c['contractNo']}'),
                        subtitle: Text('${c['name']}'),
                        trailing: Column(
                          mainAxisAlignment: MainAxisAlignment.center,
                          crossAxisAlignment: CrossAxisAlignment.end,
                          children: [
                            Text(_money(c['amountCents'])),
                            Text(
                              '已回 ${_money(c['paidCents'])}',
                              style: const TextStyle(fontSize: 12, color: Colors.grey),
                            ),
                          ],
                        ),
                      ),
                    )
                    .toList(),
              ),
            ],
          );
        },
      ),
    );
  }
}
