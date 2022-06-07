//! Prefetch plugin take a VFile attribute return from a node and  add the result of an prefetch function to the attribute of this node
#![allow(dead_code)]

use std::sync::Arc;
use std::io::BufReader;
use std::io::SeekFrom;
use std::fmt::Debug;

use tap::config_schema;
use tap::plugin;
use tap::plugin::{PluginInfo, PluginInstance, PluginConfig, PluginArgument, PluginResult, PluginEnvironment};
use tap::vfile::{VFile, read_utf16_exact, read_sized_utf16, read_utf16_list};
use tap::reflect::{ReflectStruct};
use tap::value::Value;
use tap::datetime::WindowsTimestamp;
use tap::error::RustructError;
use tap::tree::{TreeNodeId, TreeNodeIdSchema};

use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use schemars::{JsonSchema};
use byteorder::{LittleEndian, ReadBytesExt};
use tap_derive::Reflect;

plugin!("prefetch", "Windows", "Parse prefetch file", PrefetchPlugin, Arguments);

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Arguments
{
  #[schemars(with = "TreeNodeIdSchema")] 
  file : TreeNodeId,
}

#[derive(Debug, Serialize, Deserialize,Default)]
pub struct Results
{
}

#[derive(Default)]
pub struct PrefetchPlugin
{
}

impl PrefetchPlugin
{
  fn run(&mut self, args : Arguments, env : PluginEnvironment) -> anyhow::Result<Results>
  {
    let file_node = env.tree.get_node_from_id(args.file).ok_or(RustructError::ArgumentNotFound("file"))?;
    let data = file_node.value().get_value("data").ok_or(RustructError::ValueNotFound("data"))?;
    let data_builder = data.try_as_vfile_builder().ok_or(RustructError::ValueTypeMismatch)?;
    let file = data_builder.open()?;

    let mut file = BufReader::new(file); 
    let prefetch = match Prefetch::from_file(&mut file)
    {
       Ok(prefetch) => prefetch,
       Err(err) => { file_node.value().add_attribute(self.name(), None, None); return Err(err) },
    };
      
    file_node.value().add_attribute("prefetch", Arc::new(prefetch), None);

    Ok(Results{})
  }
}

/**
 *   Prefetch parser
 */
#[derive(Debug, Reflect)] //reflect ...
pub struct Prefetch
{
  pub header : Arc<PrefetchHeader>,
  pub file_information : Arc<FileInformation>,
  pub volume_information : Arc<VolumeInformation>,
  pub files : Vec<String>,
  pub volumes : Vec<String>,
}

impl Prefetch
{
  pub fn from_file<T : VFile>(file : &mut T) -> anyhow::Result<Prefetch>
  {
    let prefetch_header = PrefetchHeader::from_reader(file)?;

    let file_information = match prefetch_header.version
    {
      PrefetchVersion::WindowsVista => FileInformation::vista_from_reader(file)?,
      PrefetchVersion::WindowsXP => FileInformation::xp_from_reader(file)?, 
      PrefetchVersion::Windows8 => FileInformation::w8_from_reader(file)?,
      //PrefetchVersion::Windows10, windows 10 is compressed in lzxpress ! must handle that case
      //create an other plugin or first decompress and run this one 
      _ => return Err(RustructError::Unknown("Unsupported prefetch version".into()).into()),
    };


    file.seek(SeekFrom::Start(prefetch_header.volume_information_offset as u64))?;
    let volume_information = VolumeInformation::from_reader(file)?;
 
    file.seek(SeekFrom::Start(prefetch_header.first_file_path_offset as u64))?;
    let files = read_utf16_list(file, prefetch_header.first_file_path_size as usize)?;
    
    let offset = prefetch_header.volume_information_offset  + volume_information.folder_path_offset;
    file.seek(SeekFrom::Start(offset as u64))?; 

    let mut volumes : Vec<String> = Vec::new();
    for _ in 0..volume_information.folder_path_count
    {
      let decoded = read_sized_utf16(file)?;
      volumes.push(decoded)
    }

    Ok(Prefetch{  
     header : Arc::new(prefetch_header),
     file_information : Arc::new(file_information),
     volume_information : Arc::new(volume_information),
     files,
     volumes,
    })
  } 
}

#[derive(Debug,Reflect)]
pub struct FileInformation
{
  last_execution_time : DateTime::<Utc>,
  number_of_execution : u32,
}		

impl FileInformation
{
  fn vista_from_reader<T : VFile>(file : &mut T) -> anyhow::Result<FileInformation>
  {
    file.seek(SeekFrom::Start(0x80))?;
    let last_execution_time = file.read_u64::<LittleEndian>()?; 
    let last_execution_time = WindowsTimestamp(last_execution_time).to_datetime()?;

    file.seek(SeekFrom::Start(0x98))?;
    let number_of_execution = file.read_u32::<LittleEndian>()?;

    Ok(FileInformation{
      last_execution_time, number_of_execution
    })
  }

  fn xp_from_reader<T : VFile>(file : &mut T) -> anyhow::Result<FileInformation>
  {
    file.seek(SeekFrom::Start(0x78))?;
    let last_execution_time = file.read_u64::<LittleEndian>()?; 
    let last_execution_time = WindowsTimestamp(last_execution_time).to_datetime()?;

    file.seek(SeekFrom::Start(0x90))?;
    let number_of_execution = file.read_u32::<LittleEndian>()?;

    Ok(FileInformation{
      last_execution_time, number_of_execution
    })
  }

  fn w8_from_reader<T : VFile>(file : &mut T) -> anyhow::Result<FileInformation>
  {
    file.seek(SeekFrom::Start(0x80))?;
    let last_execution_time = file.read_u64::<LittleEndian>()?; 
    let last_execution_time = WindowsTimestamp(last_execution_time).to_datetime()?;

    file.seek(SeekFrom::Start(0xD0))?;
    let number_of_execution = file.read_u32::<LittleEndian>()?;

    Ok(FileInformation{
      last_execution_time, number_of_execution
    })
  }

  pub fn last_execution_time(&self) -> DateTime::<Utc>
  {
    self.last_execution_time
  }
 
  pub fn number_of_execution(&self) -> u32
  {
    self.number_of_execution
  }
}


#[derive(Debug, Reflect)]
pub struct VolumeInformation
{
  #[reflect(skip)]
  volume_path_offset : u32,
  #[reflect(skip)]
  volume_path_size : u32,
  volume_creation_date: DateTime<Utc>,
  volume_serial_number : u32,
  #[reflect(skip)]
  blob1_offset : u32,
  #[reflect(skip)]
  blob1_size : u32,
  #[reflect(skip)]
  folder_path_offset : u32,
  #[reflect(skip)]
  folder_path_count : u32,
}

impl VolumeInformation
{
  pub fn from_reader<T : VFile>(file : &mut T) -> anyhow::Result<VolumeInformation>
  {
    let volume_path_offset = file.read_u32::<LittleEndian>()?; 
    let volume_path_size = file.read_u32::<LittleEndian>()?;
    let volume_creation_date = file.read_u64::<LittleEndian>()?; 
    let volume_creation_date = WindowsTimestamp(volume_creation_date).to_datetime()?;
    let volume_serial_number = file.read_u32::<LittleEndian>()?;
    let blob1_offset = file.read_u32::<LittleEndian>()?;
    let blob1_size = file.read_u32::<LittleEndian>()?;
    let folder_path_offset = file.read_u32::<LittleEndian>()?; 
    let folder_path_count = file.read_u32::<LittleEndian>()?;

    Ok(VolumeInformation{
      volume_path_offset, volume_path_size, volume_creation_date, volume_serial_number,
      blob1_offset, blob1_size,
      folder_path_offset, folder_path_count,
    })
  }
}

#[derive(Debug, Reflect)]
pub struct PrefetchHeader
{
  #[reflect(skip)]
  version : PrefetchVersion,  //offset 0
  #[reflect(skip)]
  signature : String,        //offset 8
  file_size : u32,            //offset 0xc
  file_name : String,         //0x10 + 0x3c/60 ?
  hash : u32,                 //0x4c ?

  first_file_path_offset : u32, //0x64
  first_file_path_size : u32,  //0x68
  volume_information_offset : u32, //0x6c
}

#[derive(Debug)]
enum PrefetchVersion
{
  WindowsXP,
  WindowsVista,
  Windows8,
  Windows10,
}

impl PrefetchHeader
{
  pub fn from_reader<T : VFile>(file: &mut T) -> anyhow::Result<PrefetchHeader>
  {
    let version = match file.read_u32::<LittleEndian>()?
    {
      0x11 => PrefetchVersion::WindowsXP,
      0x17 => PrefetchVersion::WindowsVista,
      0x1a => PrefetchVersion::Windows8,
      0x30 => PrefetchVersion::Windows10,
      _ => return Err(RustructError::Unknown("Can't match Prefetch version".into()).into()),
    };  
  
    let mut signature: [u8; 4] = [0; 4];
    file.read_exact(&mut signature)?;
    let signature = std::str::from_utf8(&signature)?.to_string();

    file.seek(SeekFrom::Current(4))?; //XXX check seek return value
    let file_size = file.read_u32::<LittleEndian>()?;
    let file_name = read_utf16_exact(file, 60)?;
    let hash = file.read_u32::<LittleEndian>()?;

    file.seek(SeekFrom::Start(0x64))?;
    let first_file_path_offset = file.read_u32::<LittleEndian>()?;
    let first_file_path_size = file.read_u32::<LittleEndian>()?;
    let volume_information_offset = file.read_u32::<LittleEndian>()?;

    Ok(PrefetchHeader{version, signature, file_size, file_name, hash,
      first_file_path_offset, first_file_path_size, volume_information_offset})
  }
}
