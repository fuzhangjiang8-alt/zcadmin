// =============================================================================
// 商机详情：阶段/金额 + 转化合同。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';

class OpportunityDetailPage extends StatefulWidget {
  const OpportunityDetailPage({
    super.key,
    required this.api,
    required this.opportunityId,
  });

  final ApiClient api;
  final String opportunityId;

  @override
  State<OpportunityDetailPage> createState() => _OpportunityDetailPageState();
}

class _OpportunityDetailPageState extends State<OpportunityDetailPage> {
  late final Future<List<dynamic>> _future = Future.wait([
    widget.api.opportunity(widget.opportunityId),
    widget.api.opportunityContracts(widget.opportunityId),
    widget.api.opportunityStageHistory(widget.opportunityId),
  ]);

  String _money(dynamic cents) {
    final value = (cents as num? ?? 0) / 100;
    return '¥${value.toStringAsFixed(2)}';
  }

  String _stage(String? stage) =>
      const {
        'initial': '初步接触',
        'proposal': '方案/报价',
        'negotiation': '谈判',
        'won': '赢单',
        'lost': '输单',
      }[stage] ??
      (stage ?? '创建');

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('商机详情')),
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
          final opp = data[0] as Map<String, dynamic>;
          final contracts = (data[1] as List).cast<Map<String, dynamic>>();
          final history = (data[2] as List).cast<Map<String, dynamic>>();

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
                        '${opp['name']}',
                        style: Theme.of(context).textTheme.titleLarge,
                      ),
                      const SizedBox(height: 8),
                      Text('阶段：${opp['stage']}'),
                      Text('金额：${_money(opp['amountCents'])}'),
                      if (opp['expectedClose'] != null)
                        Text('预计成交：${opp['expectedClose']}'),
                    ],
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.only(top: 20, bottom: 8),
                child: Text(
                  '转化合同（${contracts.length}）',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ),
              if (contracts.isEmpty)
                const Text('暂无合同', style: TextStyle(color: Colors.grey))
              else
                ...contracts.map(
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
                ),
              Padding(
                padding: const EdgeInsets.only(top: 20, bottom: 8),
                child: Text(
                  '阶段历史（${history.length}）',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ),
              if (history.isEmpty)
                const Text('暂无阶段记录', style: TextStyle(color: Colors.grey))
              else
                ...history.map(
                  (h) => ListTile(
                    dense: true,
                    title: Text(
                      '${_stage(h['fromStage'] as String?)} → ${_stage(h['toStage'] as String?)}',
                    ),
                    subtitle: Text('${h['changedAt']}'),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }
}
