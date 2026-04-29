"""Test IFRS 16 conversion for Royal BAM Group FY2025."""
from kb.ifrs import IFRSAdjustmentInput, convert_ifrs_to_us_gaap, format_bridge

# Royal BAM Group FY2025 — extracted from Annual Report Notes 10 & 15.3
# All amounts in EUR thousands

revenue = 7_039_900
ebit = 238_195
da_total = 157_791
ebitda = ebit + da_total  # 395,986

inputs = IFRSAdjustmentInput(
    rou_depreciation=100_979,
    lease_interest=12_727,
    short_term_rent=63_470,
    reported_ebit=ebit,
    reported_ebitda=ebitda,
    reported_ebita=ebit,
    accounting_standard='IFRS',
)

notes_ref = {
    'rou_depr': 'Note 15.3 - Depreciation expense of right-of-use assets (p.192)',
    'lease_int': 'Note 10 - Interest expense on lease liabilities (p.183)',
    'short_term': 'Note 15.3 - Rent expenses short term leases, practical expedient (p.192)',
    'lease_liab': 'Note 15.2 - Lease liabilities movement schedule (p.192)',
    'rou_assets': 'Note 15.1 - Right-of-use assets (p.191)',
    'bs': 'Consolidated statement of financial position (p.164)',
}

out = convert_ifrs_to_us_gaap(inputs, revenue=revenue)

# Print the full bridge
print(format_bridge(
    inputs, out, revenue=revenue,
    company="Royal BAM Group N.V.",
    period="FY2025",
    notes_ref=notes_ref,
))
