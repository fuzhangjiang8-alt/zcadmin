// =============================================================================
// 合同列表页。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'contract_detail_page.dart';

class ContractsPage extends StatefulWidget {
  const ContractsPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<ContractsPage> createState() => _ContractsPageState();
}

class _ContractsPageState extends State<ContractsPage> {
  late Future<List<Map<String, dynamic>>> _future = widget.api.contractList();

  Future<void> _refresh() async {
    setState(() => _future = widget.api.contractList());
    await _future;
  }

  String _money(dynamic cents) {
    final value = (cents as num? ?? 0) / 100;
    return '¥${value.toStringAsFixed(2)}';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('合同'),
        actions: [
          IconButton(
            onPressed: () =>
                setState(() => _future = widget.api.contractList()),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: RefreshIndicator(
        onRefresh: _refresh,
        child: FutureBuilder<List<Map<String, dynamic>>>(
        future: _future,
        builder: (context, snapshot) {
          if (snapshot.connectionState != ConnectionState.done) {
            return const Center(child: CircularProgressIndicator());
          }
          if (snapshot.hasError) {
            return Center(child: Text(snapshot.error.toString()));
          }
          final rows = snapshot.data ?? const [];
          if (rows.isEmpty) {
            return const Center(child: Text('暂无合同'));
          }
          return ListView.separated(
            itemCount: rows.length,
            separatorBuilder: (_, __) => const Divider(height: 1),
            itemBuilder: (context, index) {
              final c = rows[index];
              return ListTile(
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
                onTap: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => ContractDetailPage(
                      api: widget.api,
                      contractId: '${c['id']}',
                    ),
                  ),
                ),
              );
            },
          );
        },
      ),
      ),
    );
  }
}
