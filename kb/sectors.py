"""
Sector frameworks ported from knowledge-base/sectors/*.md.
Sector detection + valuation ranges + key metrics.
"""

from dataclasses import dataclass, field
from typing import Optional


@dataclass
class SectorFramework:
    code: str
    name: str
    primary_multiple: str  # e.g., "EV/EBITDA"
    secondary_multiples: list[str] = field(default_factory=list)
    sector_ranges: dict = field(default_factory=dict)  # sub_sector -> {low, high}
    key_metrics: list[str] = field(default_factory=list)
    detection_keywords: list[str] = field(default_factory=list)
    ev_bridge_notes: str = ""
    deal_premium_range: str = ""  # e.g., "20-35%"


SECTORS = {
    "TMT": SectorFramework(
        code="TMT",
        name="Technology, Media & Telecommunications",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/Revenue", "P/E"],
        sector_ranges={
            "software_saas": {"low": 15, "high": 40, "metric": "EV/Revenue"},
            "hardware": {"low": 8, "high": 18, "metric": "EV/EBITDA"},
            "media_streaming": {"low": 8, "high": 20, "metric": "EV/EBITDA"},
            "telecom": {"low": 5, "high": 9, "metric": "EV/EBITDA"},
        },
        key_metrics=["Revenue Growth", "Gross Margin", "ARR/MRR (SaaS)", "CAC Payback", "Net Retention"],
        ev_bridge_notes="Stock-based compensation: add to EV bridge if material. RSUs dilute shares per TSM.",
        deal_premium_range="25-40% (strategic), 15-25% (financial)",
        detection_keywords=[
            "technology", "software", "SaaS", "platform", "media", "streaming",
            "telecom", "digital", "cloud", "AI", "data", "semiconductor", "chip",
            "e-commerce", "social media", "Apple", "Microsoft", "Google", "Alphabet",
            "Amazon", "Meta", "Netflix", "Salesforce", "Adobe", "Oracle", "SAP",
            "Snowflake", "Palantir", "Uber", "Airbnb", "Disney", "Warner",
            "Comcast", "AT&T", "Verizon", "T-Mobile", "Sony", "Samsung",
        ],
    ),
    "FIG": SectorFramework(
        code="FIG",
        name="Financial Institutions Group",
        primary_multiple="P/E",  # Banks use P/E, P/BV, P/TBV — NOT EV/EBITDA
        secondary_multiples=["P/BV", "P/TBV", "ROE", "ROTCE"],
        sector_ranges={
            "large_cap_banks": {"low": 8, "high": 14, "metric": "P/E"},
            "regional_banks": {"low": 7, "high": 12, "metric": "P/E"},
            "investment_banks": {"low": 8, "high": 15, "metric": "P/E"},
            "asset_managers": {"low": 10, "high": 18, "metric": "P/E"},
            "insurance_life": {"low": 6, "high": 12, "metric": "P/E"},
            "insurance_pc": {"low": 10, "high": 18, "metric": "P/E"},
            "fintech": {"low": 3, "high": 15, "metric": "EV/Revenue"},
        },
        key_metrics=[
            "ROE", "ROTCE", "CET1 Ratio", "NIM", "Efficiency Ratio",
            "PCL Ratio", "BVPS", "TBVPS", "P/BV", "P/TBV",
        ],
        ev_bridge_notes="EV/EBITDA not meaningful for banks. Use P/E, P/BV, P/TBV. EV bridge concept does not apply cleanly.",
        deal_premium_range="15-30% (bank M&A), 20-35% (asset management)",
        detection_keywords=[
            "bank", "insurance", "financial institution", "asset management", "wealth",
            "investment bank", "broker", "REIT", "specialty finance", "fintech",
            "JPMorgan", "Goldman", "Morgan Stanley", "Bank of America", "Wells Fargo",
            "Citi", "BlackRock", "Vanguard", "State Street", "Capital One",
            "Charles Schwab", "MetLife", "Prudential", "Aflac", "Progressive",
            "Allstate", "Chubb", "Berkshire Hathaway", "HDFC", "ICICI",
            "SBI", "Kotak", "Axis",
        ],
    ),
    "INDUSTRIALS": SectorFramework(
        code="INDUSTRIALS",
        name="Industrials",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/Revenue", "P/E"],
        sector_ranges={
            "capital_goods": {"low": 8, "high": 14, "metric": "EV/EBITDA"},
            "building_products": {"low": 8, "high": 16, "metric": "EV/EBITDA"},
            "aerospace_defense": {"low": 10, "high": 18, "metric": "EV/EBITDA"},
            "machinery": {"low": 7, "high": 13, "metric": "EV/EBITDA"},
            "transportation": {"low": 5, "high": 10, "metric": "EV/EBITDA"},
        },
        key_metrics=["EBITDA Margin", "Backlog", "Book-to-Bill", "ROIC", "FCF Conversion"],
        ev_bridge_notes="Pension common in older industrials — use R-015 (notes section). Operating leases material for transportation cos.",
        deal_premium_range="20-35% (strategic), 15-25% (financial)",
        detection_keywords=[
            "industrial", "manufacturing", "machinery", "aerospace", "defense",
            "building products", "construction", "infrastructure", "engineering",
            "conglomerate", "materials", "processing", "Caterpillar", "Deere",
            "Honeywell", "GE", "Siemens", "ABB", "Emerson", "Parker Hannifin",
            "Rockwell", "Illinois Tool", "Ingersoll Rand", "Carrier",
        ],
    ),
    "OIL_GAS": SectorFramework(
        code="OIL_GAS",
        name="Oil & Gas",
        primary_multiple="EV/EBITDAX",
        secondary_multiples=["EV/Reserves", "EV/Daily Production", "NAV"],
        sector_ranges={
            "integrated": {"low": 4, "high": 8, "metric": "EV/EBITDA"},
            "e_and_p": {"low": 3, "high": 7, "metric": "EV/EBITDA"},
            "oilfield_services": {"low": 5, "high": 12, "metric": "EV/EBITDA"},
            "refining": {"low": 4, "high": 9, "metric": "EV/EBITDA"},
        },
        key_metrics=["Production (boe/d)", "Reserves", "DD&A Rate", "Lifting Cost/boe", "Recycle Ratio"],
        ev_bridge_notes="Asset retirement obligations (ARO) treated as debt. Unproved reserves excluded from NAV.",
        deal_premium_range="15-30% (strategic), 10-20% (E&P consolidation)",
        detection_keywords=[
            "oil", "gas", "petroleum", "exploration", "production", "refining",
            "upstream", "downstream", "LNG", "pipeline", "hydrocarbon",
            "Exxon", "Chevron", "Shell", "BP", "TotalEnergies", "ConocoPhillips",
            "EOG", "Pioneer", "Diamondback", "Hess", "Marathon", "Valero",
        ],
    ),
    "HEALTHCARE": SectorFramework(
        code="HEALTHCARE",
        name="Healthcare & Biotech",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/Revenue", "P/E", "DCF (biotech)"],
        sector_ranges={
            "large_pharma": {"low": 8, "high": 14, "metric": "EV/EBITDA"},
            "biotech": {"low": 0, "high": 0, "metric": "DCF/EV/Revenue (pre-revenue)"},
            "medtech": {"low": 12, "high": 22, "metric": "EV/EBITDA"},
            "healthcare_services": {"low": 8, "high": 15, "metric": "EV/EBITDA"},
        },
        key_metrics=["Pipeline Value", "Patent Expiry Timeline", "R&D as % Revenue", "FDA Approval Status"],
        ev_bridge_notes="R&D capitalization differences IFRS vs US GAAP. In-process R&D from acquisitions may be expensed.",
        deal_premium_range="30-50% (biotech), 20-35% (pharma/medtech)",
        detection_keywords=[
            "health", "pharma", "biotech", "pharmaceutical", "biotechnology",
            "medical", "device", "hospital", "healthcare", "life sciences",
            "CRO", "CMO", "gene", "therapy", "J&J", "Pfizer", "Merck",
            "Eli Lilly", "Roche", "Novartis", "AstraZeneca", "BMS",
            "Gilead", "Amgen", "Biogen", "Moderna", "Illumina",
        ],
    ),
    "CONSUMER_RETAIL": SectorFramework(
        code="CONSUMER_RETAIL",
        name="Consumer / Retail",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/Revenue", "P/E"],
        sector_ranges={
            "food_beverage": {"low": 10, "high": 18, "metric": "EV/EBITDA"},
            "apparel_luxury": {"low": 8, "high": 20, "metric": "EV/EBITDA"},
            "restaurants": {"low": 10, "high": 20, "metric": "EV/EBITDA"},
            "home_improvement": {"low": 10, "high": 18, "metric": "EV/EBITDA"},
        },
        key_metrics=["Same-Store Sales", "Store Count", "E-commerce Penetration", "Gross Margin", "Inventory Turns"],
        ev_bridge_notes="Operating leases critical for retailers. Use ASC 842/IFRS 16 lease footnote (R-016).",
        deal_premium_range="20-35% (strategic), 15-25% (financial)",
        detection_keywords=[
            "retail", "consumer", "food", "beverage", "restaurant", "apparel",
            "fashion", "luxury", "cosmetics", "grocery", "supermarket",
            "convenience", "home improvement", "furniture", "Nike", "Starbucks",
            "McDonald's", "Chipotle", "Domino's", "Ulta", "Lululemon",
        ],
    ),
    "REAL_ESTATE": SectorFramework(
        code="REAL_ESTATE",
        name="Real Estate / REITs",
        primary_multiple="P/FFO",
        secondary_multiples=["P/AFFO", "NAV", "Implied Cap Rate"],
        sector_ranges={
            "residential": {"low": 14, "high": 22, "metric": "P/FFO"},
            "industrial_logistics": {"low": 18, "high": 30, "metric": "P/FFO"},
            "data_centers": {"low": 18, "high": 30, "metric": "P/FFO"},
            "cell_towers": {"low": 20, "high": 35, "metric": "P/FFO"},
            "office": {"low": 6, "high": 14, "metric": "P/FFO"},
            "retail": {"low": 10, "high": 18, "metric": "P/FFO"},
        },
        key_metrics=["FFO/sh", "AFFO/sh", "Occupancy Rate", "Cap Rate", "Debt/EBITDA"],
        ev_bridge_notes="EV/EBITDA not meaningful for REITs. Use P/FFO, NAV. Debt is operational part of the business model.",
        deal_premium_range="10-25% (varies by property type)",
        detection_keywords=[
            "real estate", "property", "REIT", "residential", "office", "retail",
            "industrial real estate", "logistics", "data center", "cell tower",
            "senior housing", "Prologis", "American Tower", "Crown Castle",
            "Digital Realty", "Equinix", "Public Storage", "Camden Property",
        ],
    ),
    "METALS_MINING": SectorFramework(
        code="METALS_MINING",
        name="Metals & Mining",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/Reserves", "EV/Production", "NAV", "DCF"],
        sector_ranges={
            "bulk_iron_ore": {"low": 4, "high": 8, "metric": "EV/EBITDA"},
            "copper_non_ferrous": {"low": 5, "high": 10, "metric": "EV/EBITDA"},
            "gold_producers": {"low": 8, "high": 14, "metric": "EV/EBITDA"},
            "silver_producers": {"low": 6, "high": 12, "metric": "EV/EBITDA"},
            "mining_services": {"low": 5, "high": 9, "metric": "EV/EBITDA"},
        },
        key_metrics=["Production Volume (Mtpa)", "C1 Cash Cost / AISC", "EBITDA/tonne", "Reserve Life (R/P)", "Grade / Recovery Rate"],
        ev_bridge_notes="ARO + Environmental Rehabilitation added as debt-like. Derivatives (hedge assets) subtracted if ITM. Royalty income streams = quasi-cash. Pension added if underfunded.",
        deal_premium_range="20-40% (major consolidation), 30-50% (gold mergers), 10-20% (financial)",
        detection_keywords=[
            "mining", "metals", "gold", "silver", "copper", "iron ore", "coal",
            "aluminum", "lithium", "cobalt", "rare earth", "precious metals",
            "Newmont", "Freeport", "Anglo American", "BHP", "Rio Tinto", "Glencore",
            "Vale", "US Steel", "Nucor", "Alcoa", "Southern Copper",
        ],
    ),
    "POWER_UTILITIES": SectorFramework(
        code="POWER_UTILITIES",
        name="Power & Utilities",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/RAB", "DCF", "P/E"],
        sector_ranges={
            "regulated_utilities": {"low": 10, "high": 15, "metric": "EV/EBITDA"},
            "transmission_distribution": {"low": 11, "high": 16, "metric": "EV/EBITDA"},
            "merchant_generation": {"low": 7, "high": 12, "metric": "EV/EBITDA"},
            "renewables_contracted": {"low": 12, "high": 20, "metric": "EV/EBITDA"},
            "renewables_merchant": {"low": 8, "high": 13, "metric": "EV/EBITDA"},
        },
        key_metrics=["Regulated Asset Base (RAB) Growth", "EBITDA Margin (30-45%)", "CapEx Intensity", "Net Debt/EBITDA", "PPA vs Merchant Mix"],
        ev_bridge_notes="Regulatory assets kept in EV. PPA liability may be added. Legacy pension underfunding common — add. Environmental provisions (coal ash) added if material.",
        deal_premium_range="10-25% (regulated merger), 15-30% (contracted renewables), 5-15% (financial)",
        detection_keywords=[
            "utility", "power", "electricity", "grid", "transmission", "distribution",
            "regulated", "wires", "nuclear", "generation", "NextEra", "Duke",
            "Southern Company", "Dominion", "American Electric Power", "Exelon",
            "Entergy", "Xcel", "Public Service", "Consolidated Edison",
        ],
    ),
    "INFRASTRUCTURE": SectorFramework(
        code="INFRASTRUCTURE",
        name="Project Finance & Infrastructure",
        primary_multiple="DCF",
        secondary_multiples=["EV/EBITDA", "EV/EBITDA minus CapEx", "RAB Multiples"],
        sector_ranges={
            "regulated_td": {"low": 8, "high": 12, "metric": "EV/EBITDA"},
            "water_wastewater": {"low": 10, "high": 15, "metric": "EV/EBITDA"},
            "toll_roads": {"low": 10, "high": 16, "metric": "EV/EBITDA"},
            "airports": {"low": 12, "high": 20, "metric": "EV/EBITDA"},
            "renewable_ppa": {"low": 8, "high": 14, "metric": "EV/EBITDA"},
            "pipelines_midstream": {"low": 8, "high": 12, "metric": "EV/EBITDA"},
            "digital_towers": {"low": 15, "high": 22, "metric": "EV/EBITDA"},
            "social_infra": {"low": 10, "high": 15, "metric": "EV/EBITDA"},
        },
        key_metrics=["Project IRR", "DSCR / LLCR", "Contracted Revenue %", "Availability Factor", "RAB / WACC"],
        ev_bridge_notes="Project finance debt is non-recourse — excluded at SPV level. Restricted cash NOT subtracted. Regulatory receivables + concession intangibles kept in EV. Decommissioning liabilities added if material.",
        deal_premium_range="10-25% (strategic utility), 5-20% (infra fund), 15-35% (greenfield), 0-15% (financial)",
        detection_keywords=[
            "infrastructure", "project finance", "PPP", "concession", "toll road",
            "airport", "port", "rail", "transport", "social infrastructure",
            "Macquarie", "Brookfield Infrastructure", "Atlantia", "Ferrovial", "VINCI",
        ],
    ),
    "LEVFIN": SectorFramework(
        code="LEVFIN",
        name="Leveraged Finance / DCM",
        primary_multiple="Credit Spread / OAS",
        secondary_multiples=["EV/EBITDA", "Debt Sizing (covenant-limited)", "Convertible Bond (Bond Floor + Option)"],
        sector_ranges={
            "ig_industrial": {"low": 6, "high": 10, "metric": "EV/EBITDA"},
            "bb_levfin": {"low": 8, "high": 12, "metric": "EV/EBITDA"},
            "b_levfin": {"low": 10, "high": 14, "metric": "EV/EBITDA"},
            "lbo_entry": {"low": 8, "high": 11, "metric": "EV/EBITDA"},
        },
        key_metrics=["Debt/EBITDA", "Interest Coverage (EBITDA/Int)", "OAS", "IRR / MoM", "FCF Conversion"],
        ev_bridge_notes="Minimum cash only. Revolver outstandings added if drawn. OID added to debt. Underwriting fees excluded. Goodwill + intangibles kept in EV.",
        deal_premium_range="LBO entry 0-15%, Take-private 20-40%, Add-on 0-10%, 363 sale 0-20%",
        detection_keywords=[
            "leveraged finance", "high yield", "HY", "investment grade", "bonds",
            "credit", "loans", "direct lending", "mezzanine", "unitranche",
            "Ares", "Blue Owl", "Golub Capital", "HPS", "Blackstone Credit", "KKR Credit",
        ],
    ),
    "DISTRESSED": SectorFramework(
        code="DISTRESSED",
        name="Distressed & Restructuring",
        primary_multiple="Liquidation Valuation (Floor)",
        secondary_multiples=["Adjusted DCF (distress premium)", "Distressed Multiples (peer with discount)", "Distressed Debt (Price = Par x Recovery %)"],
        sector_ranges={
            "cash_recovery": {"low": 100, "high": 100, "metric": "% Recovery"},
            "ar_recovery": {"low": 50, "high": 90, "metric": "% Recovery"},
            "inventory_recovery": {"low": 40, "high": 80, "metric": "% Recovery"},
            "ppe_recovery": {"low": 20, "high": 70, "metric": "% Recovery"},
        },
        key_metrics=["13-Week Cash Flow", "Recovery Rate by Tranche", "Distressed Debt Price (% par)", "Liquidation Value (floor)", "DIP / EBITDA / Total Debt Service"],
        ev_bridge_notes="Excess cash only above minimum operating. Inventory at liquidation value. Goodwill + intangibles = 0. Pension deficit in full. Environmental liabilities added if off-BS. DIP = senior in waterfall.",
        deal_premium_range="Section 363: 0-20% vs liquidation, Pre-pack: secured at par, Distressed exchange: at debt price",
        detection_keywords=[
            "distressed", "restructuring", "turnaround", "bankruptcy", "workout",
            "creditor", "default", "Chapter 11", "special situation",
            "Oaktree", "Canyon Capital", "Benefit Street", "Sixth Street", "Athene",
        ],
    ),
    "FSG": SectorFramework(
        code="FSG",
        name="Financial Sponsors Group (Private Equity)",
        primary_multiple="LBO Returns (IRR / MoM)",
        secondary_multiples=["SOTP (PE Firm)", "EV/EBITDA (Portfolio Co)", "HoldCo Valuation"],
        sector_ranges={
            "tmt_software": {"low": 10, "high": 18, "metric": "EV/EBITDA"},
            "healthcare_services": {"low": 8, "high": 14, "metric": "EV/EBITDA"},
            "industrials": {"low": 6, "high": 12, "metric": "EV/EBITDA"},
            "consumer_retail": {"low": 6, "high": 12, "metric": "EV/EBITDA"},
            "financial_services": {"low": 8, "high": 16, "metric": "EV/EBITDA"},
        },
        key_metrics=["IRR (fund + deal)", "MoM", "Debt/EBITDA at entry", "EBITDA Growth Rate", "TVPI / DPI"],
        ev_bridge_notes="Minimum cash kept (operating buffer). Existing debt repaid at close, replaced with LBO debt. Transaction fees + financing fees added to Investor Equity. Management rollover = equity source.",
        deal_premium_range="LBO public 20-40%, LBO private 0-15%, Take-private 25-50%, Secondary buyout 10-25%",
        detection_keywords=[
            "private equity", "buyout", "sponsor", "LBO", "growth equity", "venture",
            "KKR", "Blackstone", "Carlyle", "Apollo", "Silver Lake", "TPG",
            "Advent", "Bain Capital", "Golden Gate", "Hellman & Friedman",
        ],
    ),
    "ECM": SectorFramework(
        code="ECM",
        name="Equity Capital Markets",
        primary_multiple="P/E",
        secondary_multiples=["EV/Revenue", "Post-Money Equity Value", "Debt vs Equity Decision"],
        sector_ranges={
            "ipo_tech": {"low": 20, "high": 50, "metric": "P/E"},
            "ipo_mature": {"low": 10, "high": 18, "metric": "P/E"},
            "follow_on_growth": {"low": 15, "high": 35, "metric": "P/E"},
            "follow_on_value": {"low": 8, "high": 15, "metric": "P/E"},
            "rights_issue": {"low": 3, "high": 8, "metric": "P/E"},
        },
        key_metrics=["Offer Price vs Indicative Range", "Primary vs Secondary Mix", "Post-Money Equity Value", "Greenshoe (15%)", "P/E Multiple"],
        ev_bridge_notes="New IPO proceeds added to equity post-IPO. Greenshoe shares factor in if exercised. Lock-up shares excluded from free float.",
        deal_premium_range="IPO 10-20% discount to talk, Follow-On 3-8% discount, PIPE 10-25% discount",
        detection_keywords=[
            "equity capital markets", "IPO", "listing", "secondary offering",
            "equity underwriting", "bookrun", "greenshoe",
            "Goldman ECM", "Morgan Stanley ECM", "JPM ECM", "Citi ECM",
        ],
    ),
    "PRIVATE_CAP": SectorFramework(
        code="PRIVATE_CAP",
        name="Private Capital Advisory",
        primary_multiple="Price/NAV",
        secondary_multiples=["DCF on NAV", "Comparable Secondary Transactions", "Loan Pricing (Private Credit)"],
        sector_ranges={
            "buyout_secondary": {"low": 85, "high": 95, "metric": "% of NAV"},
            "vc_secondary": {"low": 60, "high": 85, "metric": "% of NAV"},
            "infra_fund": {"low": 95, "high": 105, "metric": "% of NAV"},
            "credit_fund": {"low": 90, "high": 98, "metric": "% of NAV"},
            "gp_led": {"low": 100, "high": 105, "metric": "% of NAV"},
        },
        key_metrics=["DPI / TVPI / IRR", "MOIC", "LP Re-up Rate", "Secondary Price / NAV", "LP Election Rate"],
        ev_bridge_notes="LP commitments called = equity drawn. Undrawn commitments excluded. GP carry added if accrued. NAV loan = debt. Claw-back provision subtracted.",
        deal_premium_range="LP-led diversified 85-95% NAV, GP-led 100-105%, Single-Asset 90-110%, Distressed LP 50-75%",
        detection_keywords=[
            "private capital", "private advisory", "sell-side advisory", "buy-side advisory",
            "independent advisory", "Lazard", "Evercore", "Greenhill",
            "Houlihan Lokey", "Rothschild", "Perella Weinberg",
        ],
    ),
    "PRIVATE_CO": SectorFramework(
        code="PRIVATE_CO",
        name="Private Companies",
        primary_multiple="Comparable Public with Private Discount",
        secondary_multiples=["Precedent Transactions", "DCF (higher WACC)", "EV/Revenue or EV/ARR (pre-profit)"],
        sector_ranges={
            "money_business": {"low": 30, "high": 50, "metric": "% Discount"},
            "meth_business_vc": {"low": 10, "high": 25, "metric": "% Discount"},
            "meth_business_pre_ipo": {"low": 0, "high": 10, "metric": "% Discount"},
            "empire_business": {"low": 5, "high": 10, "metric": "% Discount"},
        },
        key_metrics=["Normalized EBITDA (owner comp adj)", "Private Co Discount (10-30%)", "Revenue Growth / ARR", "Burn Rate / Runway", "EBITDA Margin"],
        ev_bridge_notes="Owner-related debt: include if commercial, exclude shareholder loans. Related-party receivables subtracted. Key man insurance cash value added. Personal assets excluded. Non-operating RE subtracted at FMV.",
        deal_premium_range="Strategic to Money 10-30%, Strategic to Empire 20-40%, PE to Meth 15-30%, MBO 15-30%",
        detection_keywords=[
            "private company", "family-owned", "founder-owned", "non-listed",
            "empire business", "money business", "closely held",
        ],
    ),
    "RENEWABLES": SectorFramework(
        code="RENEWABLES",
        name="Renewable Energy",
        primary_multiple="EV/EBITDA",
        secondary_multiples=["EV/MW Capacity", "LCOE-Based", "RAB Model", "EV/EBITDA minus CapEx"],
        sector_ranges={
            "solar_utility": {"low": 10, "high": 18, "metric": "EV/EBITDA"},
            "wind_onshore": {"low": 10, "high": 20, "metric": "EV/EBITDA"},
            "wind_offshore": {"low": 12, "high": 25, "metric": "EV/EBITDA"},
            "storage_bess": {"low": 15, "high": 30, "metric": "EV/EBITDA"},
            "green_hydrogen": {"low": 8, "high": 20, "metric": "EV/EBITDA"},
            "biofuels": {"low": 5, "high": 10, "metric": "EV/EBITDA"},
            "diversified_portfolio": {"low": 12, "high": 22, "metric": "EV/EBITDA"},
        },
        key_metrics=["Capacity (MW/GW) / Generation (MWh)", "PPA Remaining Term", "CAFD", "LCOE", "Merchant Exposure %"],
        ev_bridge_notes="Tax equity investments subtracted (non-operating financing). FIT receivables + grid connection rights added. CIP NOT subtracted. Project debt (non-recourse) added. PPA intangible above market added. Land leases = capital lease obligations.",
        deal_premium_range="25-40% (strategic utility), 30-50% (oil major entry), 15-25% (infra fund), 10-20% (PE)",
        detection_keywords=[
            "renewable", "solar", "wind", "green energy", "clean energy",
            "hydrogen", "biomass", "hydro", "renewables", "wind farm", "solar farm",
            "NextEra Energy", "Enphase", "SolarEdge", "First Solar", "Vestas",
            "Orsted", "RWE", "Engie", "EDP", "Iberdrola", "SSE", "National Grid",
        ],
    ),
}


# --- SECTOR DETECTION ---

def detect_sector(company_name: str, business_description: str = "") -> Optional[SectorFramework]:
    """Detect sector from company name and optional business description using keyword matching."""
    combined = (company_name + " " + business_description).lower()

    best_match = None
    best_score = 0

    for code, framework in SECTORS.items():
        score = 0
        for keyword in framework.detection_keywords:
            if keyword.lower() in combined:
                score += 1
        if score > best_score:
            best_score = score
            best_match = framework

    return best_match if best_score > 0 else None


# --- KEY METRICS BY QUERY TYPE ---

EARNINGS_METRICS_BANK = [
    "Revenue (total + by segment: NII, Trading FICC, Trading Equity, IB)",
    "EPS vs consensus",
    "Net Interest Income (NII) + NIM",
    "Provision for Credit Losses (PCL)",
    "ROE",
    "ROTCE",
    "CET1 Ratio",
    "Book Value Per Share (BVPS)",
    "Tangible Book Value Per Share (TBVPS)",
    "P/BV at current price",
    "P/TBV at current price",
    "Efficiency Ratio",
    "Capital actions",
    "Forward guidance",
    "Trading revenue breakdown (FICC vs Equity)",
    "IB revenue breakdown (Advisory vs Equity UW vs Debt UW)",
]

EARNINGS_METRICS_GENERAL = [
    "Revenue",
    "EBITDA",
    "EBIT",
    "EPS vs consensus",
    "Gross Margin",
    "EBITDA Margin",
    "Net Income",
    "Free Cash Flow",
    "Revenue Growth YoY",
    "Forward Guidance",
]
