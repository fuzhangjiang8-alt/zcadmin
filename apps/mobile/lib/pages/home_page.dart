// =============================================================================
// 底部导航：客户 / 商机。
// =============================================================================

import 'package:flutter/material.dart';

import '../api_client.dart';
import 'contracts_page.dart';
import 'dashboard_page.dart';
import 'customers_page.dart';
import 'opportunities_page.dart';
import 'payments_page.dart';

class HomePage extends StatefulWidget {
  const HomePage({super.key, required this.api});
  final ApiClient api;

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  int _index = 0;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: IndexedStack(
        index: _index,
        children: [
          DashboardPage(api: widget.api),
          CustomersPage(api: widget.api),
          OpportunitiesPage(api: widget.api),
          ContractsPage(api: widget.api),
          PaymentsPage(api: widget.api),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (index) => setState(() => _index = index),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.dashboard), label: '看板'),
          NavigationDestination(icon: Icon(Icons.people), label: '客户'),
          NavigationDestination(icon: Icon(Icons.trending_up), label: '商机'),
          NavigationDestination(icon: Icon(Icons.description), label: '合同'),
          NavigationDestination(icon: Icon(Icons.payments), label: '回款'),
        ],
      ),
    );
  }
}
