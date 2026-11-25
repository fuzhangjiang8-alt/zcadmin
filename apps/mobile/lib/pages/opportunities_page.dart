// =============================================================================
// 商机列表页。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'opportunity_detail_page.dart';

class OpportunitiesPage extends StatefulWidget {
  const OpportunitiesPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<OpportunitiesPage> createState() => _OpportunitiesPageState();
}

class _OpportunitiesPageState extends State<OpportunitiesPage> {
  late Future<List<Map<String, dynamic>>> _future =
      widget.api.opportunityList();

  Future<void> _refresh() async {
    setState(() => _future = widget.api.opportunityList());
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
        title: const Text('商机'),
        actions: [
          IconButton(
            onPressed: () =>
                setState(() => _future = widget.api.opportunityList()),
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
            return const Center(child: Text('暂无商机'));
          }
          return ListView.separated(
            itemCount: rows.length,
            separatorBuilder: (_, __) => const Divider(height: 1),
            itemBuilder: (context, index) {
              final o = rows[index];
              return ListTile(
                title: Text('${o['name']}'),
                subtitle: Text('${o['stage']}'),
                trailing: Text(_money(o['amountCents'])),
                onTap: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => OpportunityDetailPage(
                      api: widget.api,
                      opportunityId: '${o['id']}',
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
