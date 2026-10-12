//! A Parquet file read for showing: what its columns are called and
//! hold, how many rows it has, and the first of them as a table's cells.
//! Read only. The row count and the columns come from the file's foot,
//! so a file of any size says them without being read through.

use std::path::Path;

use ::parquet::basic::{ConvertedType, LogicalType, Type as Physical};
use ::parquet::file::reader::{FileReader, SerializedFileReader};
use ::parquet::record::Field;
use ::parquet::schema::types::Type;

/// A Parquet file as the pane shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    /// Each column's name and what it holds, in the file's order.
    pub columns: Vec<(String, String)>,
    /// The first rows, a cell a column.
    pub rows: Vec<Vec<String>>,
    /// How many rows the file has.
    pub total: usize,
}

/// What a column holds, in a word: its meaning where the file gives one
/// (a string, a date), else how it is stored.
fn kind(of: &Type) -> String {
    let info = of.get_basic_info();
    if let Some(logical) = info.logical_type_ref() {
        let word = match logical {
            LogicalType::String | LogicalType::Enum | LogicalType::Json => "string".to_string(),
            LogicalType::Uuid => "uuid".into(),
            LogicalType::Date => "date".into(),
            LogicalType::Time { .. } => "time".into(),
            LogicalType::Timestamp { .. } => "timestamp".into(),
            LogicalType::Decimal(d) => format!("decimal({},{})", d.precision, d.scale),
            LogicalType::Integer(i) => format!("{}int{}", if i.is_signed { "" } else { "u" }, i.bit_width),
            LogicalType::List => "list".into(),
            LogicalType::Map => "map".into(),
            LogicalType::Float16 => "float16".into(),
            _ => String::new(),
        };
        if !word.is_empty() {
            return word;
        }
    }
    match info.converted_type() {
        ConvertedType::UTF8 | ConvertedType::ENUM | ConvertedType::JSON => return "string".into(),
        ConvertedType::DATE => return "date".into(),
        ConvertedType::TIME_MILLIS | ConvertedType::TIME_MICROS => return "time".into(),
        ConvertedType::TIMESTAMP_MILLIS | ConvertedType::TIMESTAMP_MICROS => return "timestamp".into(),
        ConvertedType::DECIMAL => return "decimal".into(),
        ConvertedType::LIST => return "list".into(),
        ConvertedType::MAP | ConvertedType::MAP_KEY_VALUE => return "map".into(),
        _ => {}
    }
    if !of.is_primitive() {
        return "struct".into();
    }
    match of.get_physical_type() {
        Physical::BOOLEAN => "bool",
        Physical::INT32 => "int32",
        Physical::INT64 => "int64",
        Physical::INT96 => "timestamp",
        Physical::FLOAT => "float",
        Physical::DOUBLE => "double",
        Physical::BYTE_ARRAY | Physical::FIXED_LEN_BYTE_ARRAY => "bytes",
    }
    .into()
}

/// A cell as it is shown: nothing for a null, a string without its
/// quotes, anything else as the reader writes it.
fn cell(field: &Field) -> String {
    match field {
        Field::Null => String::new(),
        Field::Str(text) => text.clone(),
        Field::Bytes(bytes) => format!("{} bytes", bytes.len()),
        other => other.to_string(),
    }
}

/// The file at `path`: its columns, its row count, and its first
/// `most_rows` rows. A row that cannot be read ends the rows there.
pub fn read(path: &Path, most_rows: usize) -> Result<Table, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let reader = SerializedFileReader::new(file).map_err(|e| e.to_string())?;
    let meta = reader.metadata().file_metadata();
    let total = meta.num_rows().max(0) as usize;
    let columns: Vec<(String, String)> = meta.schema().get_fields().iter().map(|f| (f.name().to_string(), kind(f))).collect();
    let mut rows = Vec::new();
    for row in reader.get_row_iter(None).map_err(|e| e.to_string())?.take(most_rows) {
        let Ok(row) = row else { break };
        rows.push(row.get_column_iter().map(|(_, field)| cell(field)).collect());
    }
    Ok(Table { columns, rows, total })
}
