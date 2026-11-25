// =============================================================================
// 回款列表：关键字 + 日期范围筛选，点击进入合同详情。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'contract_detail_page.dart';

class PaymentsPage extends StatefulWidget {
  const PaymentsPage({super.key, required this.api});
  final ApiClient api;

  @override
  State<PaymentsPage> createState() => _PaymentsPageState();
}

class _PaymentsPageState extends State<PaymentsPage> {
  final _keywordController = TextEditingController();
  late Future<List<Map<String, dynamic>>> _future = widget.api.paymentList();
  DateTimeRange? _range;

  @override
  void dispose() {
    _keywordController.dispose();
    super.dispose();
  }

  Future<void> _reload() async {
    final keyword = _keywordController.text.trim();
    setState(() {
      _future = widget.api.paymentList(
        keyword: keyword.isEmpty ? null : keyword,
        from: _range?.start,
        to: _range?.end,
      );
    });
    await _future;
  }

  Future<void> _pickRange() async {
    final now = DateTime.now();
    final picked = await showDateRangePicker(
      context: context,
      firstDate: DateTime(now.year - 5),
      lastDate: DateTime(now.year + 1, 12, 31),
      initialDateRange: _range,
    );
    if (picked != null) {
      setState(() => _range = picked);
      await _reload();
    }
  }

  String _money(dynamic cents) {
    final value = (cents as num? ?? 0) / 100;
    return '¥${value.toStringAsFixed(2)}';
  }

  String _rangeText(DateTimeRange range) {
    String d(DateTime v) =>
        '${v.year}-${v.month.toString().padLeft(2, '0')}-${v.day.toString().padLeft(2, '0')}';
    return '${d(range.start)} ~ ${d(range.end)}';
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _keywordController,
                  decoration: const InputDecoration(
                    isDense: true,
                    hintText: '搜索回款编号',
                    prefixIcon: Icon(Icons.search),
                  ),
                  textInputAction: TextInputAction.search,
                  onSubmitted: (_) => _reload(),
                ),
              ),
              IconButton(
                tooltip: '选择日期范围',
                onPressed: _pickRange,
                icon: const Icon(Icons.date_range),
              ),
              if (_range != null)
                IconButton(
                  tooltip: '清除日期',
                  onPressed: () {
                    setState(() => _range = null);
                    _reload();
                  },
                  icon: const Icon(Icons.close),
                ),
            ],
          ),
        ),
        if (_range != null)
          Align(
            alignment: Alignment.centerLeft,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Text(_rangeText(_range!), style: const TextStyle(color: Colors.grey)),
            ),
          ),
        Expanded(
          child: RefreshIndicator(
            onRefresh: _reload,
            child: FutureBuilder<List<Map<String, dynamic>>>(
              future: _future,
              builder: (context, snapshot) {
                if (snapshot.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snapshot.hasError) {
                  return Center(child: Text(snapshot.error.toString()));
                }
                final rows = snapshot.data ?? [];
                if (rows.isEmpty) {
                  return const Center(child: Text('暂无回款记录'));
                }
                return ListView.separated(
                  itemCount: rows.length,
                  separatorBuilder: (_, __) => const Divider(height: 1),
                  itemBuilder: (context, index) {
                    final p = rows[index];
                    return ListTile(
                      title: Text('${p['paymentNo']}'),
                      subtitle: Text('${p['paidAt']} · ${p['method']}'),
                      trailing: Text(
                        _money(p['amountCents']),
                        style: const TextStyle(fontWeight: FontWeight.w600),
                      ),
                      onTap: () {
                        Navigator.of(context).push(
                          MaterialPageRoute(
                            builder: (_) => ContractDetailPage(
                              api: widget.api,
                              contractId: '${p['contractId']}',
                            ),
                          ),
                        );
                      },
                    );
                  },
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}
