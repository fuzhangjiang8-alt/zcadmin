// =============================================================================
// 客户列表页。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'customer_detail_page.dart';

class CustomersPage extends StatefulWidget {
  const CustomersPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<CustomersPage> createState() => _CustomersPageState();
}

class _CustomersPageState extends State<CustomersPage> {
  late Future<List<Map<String, dynamic>>> _future = widget.api.customers();

  Future<void> _refresh() async {
    setState(() => _future = widget.api.customers());
    await _future;
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('客户'),
        actions: [
          IconButton(
            onPressed: () => setState(() => _future = widget.api.customers()),
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
            return const Center(child: Text('暂无客户'));
          }
          return ListView.separated(
            itemCount: rows.length,
            separatorBuilder: (_, __) => const Divider(height: 1),
            itemBuilder: (context, index) {
              final c = rows[index];
              return ListTile(
                title: Text('${c['name']}'),
                subtitle: Text('${c['code']} · ${c['level']}'),
                trailing: Text('${c['status']}'),
                onTap: () => Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => CustomerDetailPage(
                      api: widget.api,
                      customerId: '${c['id']}',
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
